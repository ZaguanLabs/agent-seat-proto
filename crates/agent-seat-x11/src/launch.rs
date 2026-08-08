//! Bounded XDG catalog discovery and shell-free desktop-entry launch.

use std::collections::{HashMap, HashSet};
use std::env;
use std::fs::{self, File};
use std::io::Read as _;
use std::num::NonZeroU64;
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use agent_seat_proto::{
    ApplicationDescriptor, ApplicationId, ApplicationListRequest, ApplicationPage, BoundedList,
    ErrorCode, LaunchToken, Name, Retry,
};

use crate::config::LaunchPolicy;

const MAX_CATALOG_ENTRIES: usize = 4_096;
const MAX_SCANNED_PATHS: usize = 16_384;
const MAX_DIRECTORY_DEPTH: usize = 16;
const MAX_DESKTOP_BYTES: u64 = 64 * 1024;
const MAX_ENTRY_KEYS: usize = 256;
const MAX_EXEC_ARGUMENTS: usize = 128;
const MAX_DATA_ROOTS: usize = 32;
const MAX_ACTIVE_CHILDREN: usize = 64;

pub(crate) struct SessionCatalog {
    cached: Option<Vec<ApplicationDescriptor>>,
}

impl SessionCatalog {
    pub(crate) const fn new() -> Self {
        Self { cached: None }
    }

    pub(crate) fn list(
        &mut self,
        request: ApplicationListRequest,
        policy: &LaunchPolicy,
    ) -> Result<ApplicationPage, Failure> {
        if request.cursor == 0 {
            self.cached = Some(
                discover(policy)?
                    .into_iter()
                    .map(|entry| entry.descriptor)
                    .collect(),
            );
        }
        let catalog = self
            .cached
            .as_ref()
            .ok_or_else(|| Failure::invalid("application cursor has no session catalog"))?;
        let start = usize::try_from(request.cursor)
            .map_err(|_| Failure::invalid("application cursor cannot be represented"))?;
        if start > catalog.len() {
            return Err(Failure::invalid(
                "application cursor is outside the catalog",
            ));
        }
        let end = start
            .saturating_add(usize::from(request.limit))
            .min(catalog.len());
        let next_cursor = (end < catalog.len())
            .then(|| u32::try_from(end))
            .transpose()
            .map_err(|_| Failure::internal("application cursor space is exhausted"))?;
        Ok(ApplicationPage {
            applications: BoundedList::new(catalog[start..end].to_vec())
                .map_err(|_| Failure::internal("application page exceeded its public bound"))?,
            next_cursor,
        })
    }
}

pub(crate) struct LaunchSupervisor {
    next_token: AtomicU64,
    children: Mutex<Vec<Child>>,
}

impl LaunchSupervisor {
    pub(crate) const fn new() -> Self {
        Self {
            next_token: AtomicU64::new(1),
            children: Mutex::new(Vec::new()),
        }
    }

    pub(crate) fn reap(&self) {
        let mut children = self
            .children
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reap_children(&mut children);
    }

    pub(crate) fn launch(
        &self,
        application: &ApplicationId,
        policy: &LaunchPolicy,
    ) -> Result<Started, Failure> {
        let entry = discover(policy)?
            .into_iter()
            .find(|entry| entry.descriptor.id == *application)
            .ok_or_else(|| Failure::refused("application is not allowed by current policy"))?;
        let numeric = self
            .next_token
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| Failure::internal("launch token space is exhausted"))?;
        let token = LaunchToken::new(
            NonZeroU64::new(numeric)
                .ok_or_else(|| Failure::internal("launch token space is exhausted"))?,
        );
        let startup_id = format!("agent-seat-x11-{}-{numeric}", std::process::id());

        let mut children = self
            .children
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        reap_children(&mut children);
        if children.len() == MAX_ACTIVE_CHILDREN {
            return Err(Failure::unavailable(
                "active application launch bound is reached",
            ));
        }
        let mut command = Command::new(&entry.program);
        command
            .args(&entry.arguments)
            .env("DESKTOP_STARTUP_ID", &startup_id)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if let Some(directory) = entry.working_directory {
            command.current_dir(directory);
        }
        let child = command
            .spawn()
            .map_err(|_| Failure::unavailable("desktop application could not be started"))?;
        children.push(child);
        Ok(Started { token, startup_id })
    }
}

pub(crate) struct Started {
    pub(crate) token: LaunchToken,
    pub(crate) startup_id: String,
}

fn reap_children(children: &mut Vec<Child>) {
    let mut index = 0;
    while index < children.len() {
        match children[index].try_wait() {
            Ok(Some(_)) => {
                let mut child = children.swap_remove(index);
                let _ = child.wait();
            }
            Ok(None) | Err(_) => index += 1,
        }
    }
}

struct CatalogEntry {
    descriptor: ApplicationDescriptor,
    program: PathBuf,
    arguments: Vec<String>,
    working_directory: Option<PathBuf>,
}

#[derive(Clone)]
struct DataRoot {
    applications: PathBuf,
    user_entry: bool,
}

fn discover(policy: &LaunchPolicy) -> Result<Vec<CatalogEntry>, Failure> {
    if !policy.allows_any() {
        return Ok(Vec::new());
    }
    let mut catalog = Vec::new();
    let mut seen = HashSet::new();
    let current_desktops = current_desktops()?;
    for root in data_roots()? {
        let mut paths = desktop_paths(&root.applications)?;
        paths.sort_unstable_by(|left, right| left.1.cmp(&right.1));
        for (path, relative) in paths {
            let Some(id) = desktop_id(&relative) else {
                continue;
            };
            if !seen.insert(id.clone()) {
                continue;
            }
            if catalog.len() == MAX_CATALOG_ENTRIES {
                return Err(Failure::too_large("application catalog exceeds its bound"));
            }
            if let Some(entry) = read_entry(&path, id, root.user_entry, &current_desktops) {
                if policy.permits(&entry.descriptor.id, root.user_entry) {
                    catalog.push(entry);
                }
            }
        }
    }
    catalog.sort_unstable_by(|left, right| left.descriptor.id.cmp(&right.descriptor.id));
    Ok(catalog)
}

fn data_roots() -> Result<Vec<DataRoot>, Failure> {
    let user = match env::var_os("XDG_DATA_HOME") {
        Some(value) if !value.is_empty() => absolute_path(value, "XDG_DATA_HOME")?,
        _ => {
            let home = env::var_os("HOME")
                .ok_or_else(|| Failure::unavailable("HOME is unavailable for XDG discovery"))?;
            absolute_path(home, "HOME")?.join(".local/share")
        }
    };
    let system = env::var_os("XDG_DATA_DIRS")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    let mut roots = Vec::with_capacity(3);
    roots.push(DataRoot {
        applications: user.join("applications"),
        user_entry: true,
    });
    for value in env::split_paths(&system) {
        if roots.len() == MAX_DATA_ROOTS {
            return Err(Failure::too_large("XDG data root count exceeds its bound"));
        }
        if !value.is_absolute() {
            return Err(Failure::unavailable(
                "XDG_DATA_DIRS contains a relative path",
            ));
        }
        let applications = value.join("applications");
        if !roots.iter().any(|root| root.applications == applications) {
            roots.push(DataRoot {
                applications,
                user_entry: false,
            });
        }
    }
    Ok(roots)
}

fn absolute_path(value: std::ffi::OsString, variable: &'static str) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else if variable == "HOME" {
        Err(Failure::unavailable("HOME is not absolute"))
    } else {
        Err(Failure::unavailable("XDG_DATA_HOME is not absolute"))
    }
}

fn current_desktops() -> Result<Vec<String>, Failure> {
    let value = env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if value.len() > 4_096 {
        return Err(Failure::too_large("XDG_CURRENT_DESKTOP exceeds its bound"));
    }
    let desktops = value
        .split(':')
        .filter(|desktop| !desktop.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if desktops.len() > MAX_DATA_ROOTS {
        return Err(Failure::too_large(
            "XDG_CURRENT_DESKTOP exceeds its item bound",
        ));
    }
    Ok(desktops)
}

fn desktop_paths(root: &Path) -> Result<Vec<(PathBuf, PathBuf)>, Failure> {
    let Ok(metadata) = fs::symlink_metadata(root) else {
        return Ok(Vec::new());
    };
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    let mut pending = vec![(root.to_path_buf(), PathBuf::new(), 0_usize)];
    let mut scanned = 0_usize;
    while let Some((directory, relative, depth)) = pending.pop() {
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            scanned = scanned
                .checked_add(1)
                .ok_or_else(|| Failure::too_large("XDG scan count overflowed"))?;
            if scanned > MAX_SCANNED_PATHS {
                return Err(Failure::too_large("XDG scan exceeds its path bound"));
            }
            let name = entry.file_name();
            let Some(name_text) = name.to_str() else {
                continue;
            };
            if name_text.is_empty() || name_text == "." || name_text == ".." {
                continue;
            }
            let child_relative = relative.join(&name);
            let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
                continue;
            };
            let file_type = metadata.file_type();
            if file_type.is_dir() && depth < MAX_DIRECTORY_DEPTH {
                pending.push((entry.path(), child_relative, depth + 1));
            } else if (file_type.is_file()
                || file_type.is_symlink()
                    && fs::metadata(entry.path()).is_ok_and(|target| target.is_file()))
                && name_text.ends_with(".desktop")
            {
                result.push((entry.path(), child_relative));
            }
        }
    }
    Ok(result)
}

fn desktop_id(relative: &Path) -> Option<ApplicationId> {
    let mut id = String::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return None;
        };
        let component = component.to_str()?;
        if !id.is_empty() {
            id.push('-');
        }
        id.push_str(component);
    }
    if id.is_empty() || !id.ends_with(".desktop") {
        return None;
    }
    ApplicationId::new(id).ok()
}

fn read_entry(
    path: &Path,
    id: ApplicationId,
    user_entry: bool,
    current_desktops: &[String],
) -> Option<CatalogEntry> {
    let path_metadata = fs::symlink_metadata(path).ok()?;
    if !(path_metadata.file_type().is_file() || path_metadata.file_type().is_symlink()) {
        return None;
    }
    let file = File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > MAX_DESKTOP_BYTES {
        return None;
    }
    if path_metadata.file_type().is_file()
        && (metadata.dev() != path_metadata.dev() || metadata.ino() != path_metadata.ino())
    {
        return None;
    }
    let capacity = usize::try_from(metadata.len()).ok()?.checked_add(1)?;
    let mut bytes = Vec::with_capacity(capacity);
    file.take(MAX_DESKTOP_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_DESKTOP_BYTES {
        return None;
    }
    let values = parse_desktop(std::str::from_utf8(&bytes).ok()?)?;
    if values.get("Type").map(String::as_str) != Some("Application")
        || boolean(&values, "Hidden")?
        || boolean(&values, "NoDisplay")?
        || boolean(&values, "Terminal")?
        || !visible_on_desktop(&values, current_desktops)
    {
        return None;
    }
    let name = localized_name(&values)?;
    let icon = values.get("Icon").map(String::as_str);
    let exec = values.get("Exec")?;
    let (program, arguments) = parse_exec(exec, &name, path, icon)?;
    if let Some(try_exec) = values.get("TryExec") {
        resolve_program(&decode_string(try_exec)?)?;
    }
    let working_directory = match values.get("Path") {
        Some(value) => Some(decode_string(value)?).filter(|value| !value.is_empty()),
        None => None,
    }
    .map(PathBuf::from);
    if working_directory
        .as_ref()
        .is_some_and(|directory| !directory.is_absolute() || !directory.is_dir())
    {
        return None;
    }
    Some(CatalogEntry {
        descriptor: ApplicationDescriptor {
            id,
            name: Name::new(name).ok()?,
            user_entry,
        },
        program,
        arguments,
        working_directory,
    })
}

fn parse_desktop(source: &str) -> Option<HashMap<String, String>> {
    if source
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r'))
    {
        return None;
    }
    let mut values = HashMap::new();
    let mut in_desktop_entry = false;
    let mut found_desktop_entry = false;
    for raw_line in source.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            if !line.ends_with(']') || line[1..line.len() - 1].contains(['[', ']']) {
                return None;
            }
            in_desktop_entry = line == "[Desktop Entry]";
            if in_desktop_entry {
                if found_desktop_entry {
                    return None;
                }
                found_desktop_entry = true;
            }
            continue;
        }
        if !found_desktop_entry {
            return None;
        }
        if !in_desktop_entry {
            continue;
        }
        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        if !valid_key(key) || values.len() == MAX_ENTRY_KEYS || values.contains_key(key) {
            return None;
        }
        values.insert(key.to_owned(), value.trim().to_owned());
    }
    found_desktop_entry.then_some(values)
}

fn valid_key(key: &str) -> bool {
    let base = key.split_once('[').map_or(key, |(base, _)| base);
    !base.is_empty()
        && base
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        && (key == base
            || key
                .strip_prefix(base)
                .is_some_and(|suffix| suffix.starts_with('[') && suffix.ends_with(']')))
}

fn boolean(values: &HashMap<String, String>, key: &str) -> Option<bool> {
    match values.get(key).map(String::as_str) {
        None | Some("false") => Some(false),
        Some("true") => Some(true),
        Some(_) => None,
    }
}

fn visible_on_desktop(values: &HashMap<String, String>, current: &[String]) -> bool {
    let only = values.get("OnlyShowIn").and_then(|value| parse_list(value));
    let not = values.get("NotShowIn").and_then(|value| parse_list(value));
    if values.contains_key("OnlyShowIn") && only.is_none()
        || values.contains_key("NotShowIn") && not.is_none()
    {
        return false;
    }
    if only.as_ref().is_some_and(|only| {
        current
            .iter()
            .all(|desktop| !only.iter().any(|entry| entry == desktop))
    }) {
        return false;
    }
    !not.is_some_and(|not| {
        current
            .iter()
            .any(|desktop| not.iter().any(|entry| entry == desktop))
    })
}

fn parse_list(value: &str) -> Option<Vec<String>> {
    value
        .split_terminator(';')
        .map(decode_string)
        .collect::<Option<Vec<_>>>()
}

fn localized_name(values: &HashMap<String, String>) -> Option<String> {
    for locale in locale_candidates() {
        if let Some(value) = values.get(&format!("Name[{locale}]")) {
            let value = decode_string(value)?;
            if !value.is_empty() {
                return Some(value);
            }
        }
    }
    decode_string(values.get("Name")?).filter(|name| !name.is_empty())
}

fn locale_candidates() -> Vec<String> {
    let locale = ["LC_ALL", "LC_MESSAGES", "LANG"]
        .into_iter()
        .find_map(|key| env::var(key).ok().filter(|value| !value.is_empty()))
        .unwrap_or_default();
    let locale = locale.split('.').next().unwrap_or_default();
    if locale.is_empty() || locale == "C" || locale == "POSIX" {
        return Vec::new();
    }
    let mut candidates = Vec::with_capacity(4);
    candidates.push(locale.to_owned());
    if let Some((without_modifier, _)) = locale.split_once('@') {
        candidates.push(without_modifier.to_owned());
    }
    if let Some((language, _)) = locale.split_once('_') {
        if let Some((_, modifier)) = locale.split_once('@') {
            candidates.push(format!("{language}@{modifier}"));
        }
        candidates.push(language.to_owned());
    }
    candidates.dedup();
    candidates
}

fn decode_string(value: &str) -> Option<String> {
    let mut decoded = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            decoded.push(character);
            continue;
        }
        decoded.push(match characters.next()? {
            's' => ' ',
            'n' => '\n',
            't' => '\t',
            'r' => '\r',
            '\\' => '\\',
            _ => return None,
        });
    }
    Some(decoded)
}

struct ExecWord {
    text: String,
    quoted: bool,
}

fn parse_exec(
    value: &str,
    name: &str,
    desktop_path: &Path,
    icon: Option<&str>,
) -> Option<(PathBuf, Vec<String>)> {
    if value.is_empty() || !value.is_ascii() {
        return None;
    }
    let value = decode_string(value)?;
    let words = exec_words(&value)?;
    let mut expanded = Vec::with_capacity(words.len());
    for word in words {
        expand_word(word, name, desktop_path, icon, &mut expanded)?;
        if expanded.len() > MAX_EXEC_ARGUMENTS {
            return None;
        }
    }
    let program = expanded.first()?;
    if program.is_empty() || program.contains('=') {
        return None;
    }
    let program = resolve_program(program)?;
    let arguments = expanded.into_iter().skip(1).collect();
    Some((program, arguments))
}

fn exec_words(value: &str) -> Option<Vec<ExecWord>> {
    let mut words = Vec::new();
    let mut text = String::new();
    let mut quoted = false;
    let mut word_quoted = false;
    let mut present = false;
    let mut closed_quote = false;
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if closed_quote && !character.is_ascii_whitespace() {
            return None;
        }
        match character {
            '"' => {
                if quoted {
                    quoted = false;
                    closed_quote = true;
                } else if present {
                    return None;
                } else {
                    quoted = true;
                    word_quoted = true;
                    present = true;
                }
            }
            '\\' => {
                if !quoted {
                    return None;
                }
                let escaped = characters.next()?;
                if !matches!(escaped, '"' | '`' | '$' | '\\') {
                    return None;
                }
                text.push(escaped);
                present = true;
            }
            character if character.is_ascii_whitespace() && !quoted => {
                if present {
                    words.push(ExecWord {
                        text: std::mem::take(&mut text),
                        quoted: word_quoted,
                    });
                    word_quoted = false;
                    present = false;
                    closed_quote = false;
                }
            }
            character if !quoted && is_exec_reserved(character) => return None,
            character => {
                text.push(character);
                present = true;
            }
        }
    }
    if quoted {
        return None;
    }
    if present {
        words.push(ExecWord {
            text,
            quoted: word_quoted,
        });
    }
    (!words.is_empty()).then_some(words)
}

const fn is_exec_reserved(character: char) -> bool {
    matches!(
        character,
        '\'' | '>' | '<' | '~' | '|' | '&' | ';' | '$' | '*' | '?' | '#' | '(' | ')' | '`'
    )
}

fn expand_word(
    word: ExecWord,
    name: &str,
    desktop_path: &Path,
    icon: Option<&str>,
    output: &mut Vec<String>,
) -> Option<()> {
    if word.quoted && word.text.contains('%') {
        return None;
    }
    match word.text.as_str() {
        "%f" | "%F" | "%u" | "%U" | "%d" | "%D" | "%n" | "%N" | "%v" | "%m" => Some(()),
        "%i" => {
            let icon = match icon {
                Some(icon) => Some(decode_string(icon)?).filter(|icon| !icon.is_empty()),
                None => None,
            };
            if let Some(icon) = icon {
                output.push("--icon".to_owned());
                output.push(icon);
            }
            Some(())
        }
        "%c" => {
            output.push(name.to_owned());
            Some(())
        }
        "%k" => {
            output.push(desktop_path.to_str()?.to_owned());
            Some(())
        }
        _ => {
            let mut literal = String::with_capacity(word.text.len());
            let mut characters = word.text.chars();
            while let Some(character) = characters.next() {
                if character != '%' {
                    literal.push(character);
                } else if characters.next()? == '%' {
                    literal.push('%');
                } else {
                    return None;
                }
            }
            output.push(literal);
            Some(())
        }
    }
}

fn resolve_program(value: &str) -> Option<PathBuf> {
    let candidate = Path::new(value);
    if candidate.is_absolute() {
        return executable_file(candidate).then(|| candidate.to_path_buf());
    }
    if candidate.components().count() != 1 {
        return None;
    }
    env::var_os("PATH").and_then(|path| {
        env::split_paths(&path)
            .take(64)
            .filter(|directory| directory.is_absolute())
            .map(|directory| directory.join(candidate))
            .find(|path| executable_file(path))
    })
}

fn executable_file(path: &Path) -> bool {
    fs::metadata(path)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
}

pub(crate) struct Failure {
    pub(crate) code: ErrorCode,
    pub(crate) retry: Retry,
    pub(crate) message: &'static str,
}

impl Failure {
    const fn refused(message: &'static str) -> Self {
        Self {
            code: ErrorCode::Refused,
            retry: Retry::Never,
            message,
        }
    }

    const fn unavailable(message: &'static str) -> Self {
        Self {
            code: ErrorCode::Unavailable,
            retry: Retry::Never,
            message,
        }
    }

    const fn invalid(message: &'static str) -> Self {
        Self {
            code: ErrorCode::InvalidArgument,
            retry: Retry::Never,
            message,
        }
    }

    const fn too_large(message: &'static str) -> Self {
        Self {
            code: ErrorCode::TooLarge,
            retry: Retry::Never,
            message,
        }
    }

    const fn internal(message: &'static str) -> Self {
        Self {
            code: ErrorCode::Internal,
            retry: Retry::Never,
            message,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_parser_expands_without_shell_interpretation() {
        let path = Path::new("/usr/share/applications/example.desktop");
        let parsed = parse_exec(
            r#"/bin/true "two words" "literal;touch" /tmp/pwn %% %c %i %k %U"#,
            "Example Name",
            path,
            Some("example-icon"),
        )
        .expect("valid strict exec");
        assert_eq!(parsed.0, Path::new("/bin/true"));
        assert_eq!(
            parsed.1,
            [
                "two words",
                "literal;touch",
                "/tmp/pwn",
                "%",
                "Example Name",
                "--icon",
                "example-icon",
                "/usr/share/applications/example.desktop",
            ]
        );
    }

    #[test]
    fn exec_parser_rejects_unknown_or_ambiguous_fields() {
        let path = Path::new("/example.desktop");
        for value in [
            "/bin/true %x",
            "/bin/true before%c",
            r#"/bin/true "%c""#,
            "/bin/true %",
            "/bin/true unsafe;argument",
            "/bin/true \"unterminated",
        ] {
            assert!(
                parse_exec(value, "Example", path, None).is_none(),
                "accepted {value:?}"
            );
        }
    }

    #[test]
    fn desktop_parser_is_strict_and_group_scoped() {
        let parsed = parse_desktop(
            "# comment\n[Desktop Entry]\nType=Application\nName=Example\nExec=/bin/true\n\
             [Desktop Action New]\nName=Ignored\n",
        )
        .expect("valid desktop entry");
        assert_eq!(parsed.get("Name").map(String::as_str), Some("Example"));
        assert!(parse_desktop("Name=Before group\n[Desktop Entry]\n").is_none());
        assert!(parse_desktop("[Desktop Entry]\nName=A\nName=B\n").is_none());
        assert!(parse_desktop("[Desktop Entry]\nName=A\0B\n").is_none());
    }
}
