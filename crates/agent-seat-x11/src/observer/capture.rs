//! Bounded target-owned capture through the X Composite extension.

use agent_seat_proto::{
    CaptureData, CaptureFormat, CaptureReply, MAX_CAPTURE_HEIGHT, MAX_CAPTURE_PIXELS,
    MAX_CAPTURE_PNG_BYTES, MAX_CAPTURE_WIDTH, TargetRequest,
};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use x11rb::connection::Connection as _;
use x11rb::errors::ReplyError;
use x11rb::protocol::ErrorKind;
use x11rb::protocol::composite::{ConnectionExt as _, Redirect};
use x11rb::protocol::xproto::{
    ConnectionExt as _, ImageFormat, ImageOrder, MapState, VisualClass, Visualtype,
};

use super::{Failure, Observer};

const COMPOSITE_MAJOR: u32 = 0;
const COMPOSITE_MINOR: u32 = 4;
const REQUIRED_COMPOSITE_MINOR: u32 = 2;

struct RawCapture {
    width: u16,
    height: u16,
    depth: u8,
    visual: Visualtype,
    bits_per_pixel: u8,
    scanline_pad: u8,
    image_byte_order: ImageOrder,
    data: Vec<u8>,
}

impl Observer {
    pub(crate) fn capture_obscured(
        &mut self,
        request: TargetRequest,
    ) -> Result<CaptureReply, Failure> {
        let raw = self.under_server_grab(|observer| observer.read_target_pixels(request))?;
        let rgb = raw.to_rgb()?;
        let data = encode_png(raw.width, raw.height, &rgb)?;
        Ok(CaptureReply {
            target: request,
            width: raw.width,
            height: raw.height,
            format: CaptureFormat::Png,
            data,
        })
    }

    fn read_target_pixels(&mut self, request: TargetRequest) -> Result<RawCapture, Failure> {
        self.refresh()?;
        let target = self.target(request)?;
        if !self.redirected_clients.contains(&target.xid) {
            return Err(Failure::unavailable(
                "capture target is not held in provider-owned off-screen storage",
            ));
        }
        let attributes = self
            .connection
            .get_window_attributes(target.xid)
            .map_err(|_| Failure::unavailable("cannot inspect capture target"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot inspect capture target"))?;
        if attributes.map_state != MapState::VIEWABLE {
            return Err(Failure::unavailable("capture target is not viewable"));
        }
        let visual = self
            .connection
            .setup()
            .roots
            .iter()
            .flat_map(|screen| &screen.allowed_depths)
            .flat_map(|depth| &depth.visuals)
            .find(|visual| visual.visual_id == attributes.visual)
            .copied()
            .ok_or_else(|| Failure::unsupported("capture target visual is unavailable"))?;
        if visual.class != VisualClass::TRUE_COLOR {
            return Err(Failure::unsupported(
                "capture requires a TrueColor X11 visual",
            ));
        }

        self.read_redirected_pixmap(target.xid, visual)
    }

    pub(super) fn reconcile_capture_targets(&mut self, clients: &[u32]) -> Result<(), Failure> {
        self.require_composite_capture()?;
        let current = clients
            .iter()
            .copied()
            .collect::<std::collections::HashSet<_>>();
        for window in self
            .redirected_clients
            .difference(&current)
            .copied()
            .collect::<Vec<_>>()
        {
            let attributes = self
                .connection
                .get_window_attributes(window)
                .map_err(|_| Failure::unavailable("cannot inspect a departing capture target"))?
                .reply();
            match attributes {
                Ok(_) => self
                    .connection
                    .composite_unredirect_window(window, Redirect::AUTOMATIC)
                    .map_err(|_| Failure::unavailable("cannot request target unredirection"))?
                    .check()
                    .map_err(|_| Failure::unavailable("cannot unredirect a departing target"))?,
                Err(ReplyError::X11Error(error)) if error.error_kind == ErrorKind::Window => {}
                Err(_) => {
                    return Err(Failure::unavailable(
                        "cannot classify a departing capture target",
                    ));
                }
            }
            self.redirected_clients.remove(&window);
        }
        let additions = current
            .difference(&self.redirected_clients)
            .copied()
            .collect::<Vec<_>>();
        for window in additions {
            self.connection
                .composite_redirect_window(window, Redirect::AUTOMATIC)
                .map_err(|_| Failure::unavailable("cannot request target redirection"))?
                .check()
                .map_err(|_| Failure::unavailable("cannot redirect a scoped capture target"))?;
            self.redirected_clients.insert(window);
        }
        Ok(())
    }

    fn require_composite_capture(&mut self) -> Result<(), Failure> {
        if self.capture_extension_checked {
            return Ok(());
        }
        let version = self
            .connection
            .composite_query_version(COMPOSITE_MAJOR, COMPOSITE_MINOR)
            .map_err(|_| Failure::unsupported("X Composite cannot be queried"))?
            .reply()
            .map_err(|_| Failure::unsupported("X Composite is unavailable"))?;
        if version.major_version != COMPOSITE_MAJOR
            || version.minor_version < REQUIRED_COMPOSITE_MINOR
        {
            return Err(Failure::unsupported(
                "X Composite 0.2 or newer is required for obscured capture",
            ));
        }
        self.capture_extension_checked = true;
        Ok(())
    }

    fn read_redirected_pixmap(
        &self,
        window: u32,
        visual: Visualtype,
    ) -> Result<RawCapture, Failure> {
        let pixmap = self
            .connection
            .generate_id()
            .map_err(|_| Failure::unavailable("cannot allocate a capture pixmap identity"))?;
        self.connection
            .composite_name_window_pixmap(window, pixmap)
            .map_err(|_| Failure::unavailable("cannot request the target pixmap"))?
            .check()
            .map_err(|_| Failure::unavailable("cannot name the target pixmap"))?;

        let result = self.read_named_pixmap(pixmap, visual);
        let freed = self
            .connection
            .free_pixmap(pixmap)
            .map_err(|_| Failure::unavailable("cannot request capture pixmap cleanup"))
            .and_then(|cookie| {
                cookie
                    .check()
                    .map_err(|_| Failure::unavailable("cannot clean up capture pixmap"))
            });
        match (result, freed) {
            (Ok(raw), Ok(())) => Ok(raw),
            (Err(error), _) | (Ok(_), Err(error)) => Err(error),
        }
    }

    fn read_named_pixmap(&self, pixmap: u32, visual: Visualtype) -> Result<RawCapture, Failure> {
        let geometry = self
            .connection
            .get_geometry(pixmap)
            .map_err(|_| Failure::unavailable("cannot inspect the target pixmap"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot inspect the target pixmap"))?;
        validate_dimensions(geometry.width, geometry.height)?;
        let format = self
            .connection
            .setup()
            .pixmap_formats
            .iter()
            .find(|format| format.depth == geometry.depth)
            .ok_or_else(|| Failure::unsupported("capture pixmap format is unavailable"))?;
        if !matches!(format.bits_per_pixel, 16 | 24 | 32)
            || format.scanline_pad == 0
            || format.scanline_pad % 8 != 0
        {
            return Err(Failure::unsupported(
                "capture pixmap storage format is unsupported",
            ));
        }
        let image = self
            .connection
            .get_image(
                ImageFormat::Z_PIXMAP,
                pixmap,
                0,
                0,
                geometry.width,
                geometry.height,
                u32::MAX,
            )
            .map_err(|_| Failure::unavailable("cannot request capture pixels"))?
            .reply()
            .map_err(|_| Failure::unavailable("cannot read capture pixels"))?;
        if image.depth != geometry.depth {
            return Err(Failure::unavailable("capture pixel depth changed"));
        }
        Ok(RawCapture {
            width: geometry.width,
            height: geometry.height,
            depth: image.depth,
            visual,
            bits_per_pixel: format.bits_per_pixel,
            scanline_pad: format.scanline_pad,
            image_byte_order: self.connection.setup().image_byte_order,
            data: image.data,
        })
    }
}

impl Drop for Observer {
    fn drop(&mut self) {
        for window in self.redirected_clients.drain() {
            let _ = self
                .connection
                .composite_unredirect_window(window, Redirect::AUTOMATIC);
        }
        let _ = self.connection.flush();
    }
}

impl RawCapture {
    fn to_rgb(&self) -> Result<Vec<u8>, Failure> {
        if self.depth == 0 {
            return Err(Failure::unsupported("capture pixmap has no color depth"));
        }
        let pixel_bytes = usize::from(self.bits_per_pixel / 8);
        let row_bits = usize::from(self.width)
            .checked_mul(usize::from(self.bits_per_pixel))
            .ok_or_else(|| Failure::too_large("capture scanline size overflowed"))?;
        let pad = usize::from(self.scanline_pad);
        let stride = row_bits
            .div_ceil(pad)
            .checked_mul(pad)
            .and_then(|bits| bits.checked_div(8))
            .ok_or_else(|| Failure::too_large("capture scanline size overflowed"))?;
        let required = stride
            .checked_mul(usize::from(self.height))
            .ok_or_else(|| Failure::too_large("capture pixel storage overflowed"))?;
        if self.data.len() < required || self.data.len() > required.saturating_add(3) {
            return Err(Failure::unavailable(
                "capture pixel storage has an unexpected length",
            ));
        }
        let pixel_count = usize::from(self.width) * usize::from(self.height);
        let mut rgb = Vec::with_capacity(pixel_count * 3);
        for row in self.data[..required].chunks_exact(stride) {
            for pixel in row[..usize::from(self.width) * pixel_bytes].chunks_exact(pixel_bytes) {
                let value = pixel_value(pixel, self.image_byte_order);
                rgb.push(component(value, self.visual.red_mask)?);
                rgb.push(component(value, self.visual.green_mask)?);
                rgb.push(component(value, self.visual.blue_mask)?);
            }
        }
        Ok(rgb)
    }
}

fn validate_dimensions(width: u16, height: u16) -> Result<(), Failure> {
    let pixels = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| Failure::too_large("capture dimensions overflowed"))?;
    if width == 0
        || height == 0
        || width > MAX_CAPTURE_WIDTH
        || height > MAX_CAPTURE_HEIGHT
        || pixels > MAX_CAPTURE_PIXELS
    {
        return Err(Failure::too_large(
            "capture target exceeds the published image bounds",
        ));
    }
    Ok(())
}

fn pixel_value(bytes: &[u8], order: ImageOrder) -> u32 {
    if order == ImageOrder::LSB_FIRST {
        bytes
            .iter()
            .enumerate()
            .fold(0_u32, |value, (index, byte)| {
                value | (u32::from(*byte) << (index * 8))
            })
    } else {
        bytes
            .iter()
            .fold(0_u32, |value, byte| (value << 8) | u32::from(*byte))
    }
}

fn component(pixel: u32, mask: u32) -> Result<u8, Failure> {
    if mask == 0 {
        return Err(Failure::unsupported(
            "capture visual has an empty color mask",
        ));
    }
    let shift = mask.trailing_zeros();
    let maximum = mask >> shift;
    if !maximum.checked_add(1).is_some_and(u32::is_power_of_two) {
        return Err(Failure::unsupported(
            "capture visual has a noncontiguous color mask",
        ));
    }
    let value = (pixel & mask) >> shift;
    let scaled = (u64::from(value) * 255 + u64::from(maximum / 2)) / u64::from(maximum);
    u8::try_from(scaled).map_err(|_| Failure::unsupported("capture color conversion overflowed"))
}

fn encode_png(width: u16, height: u16, rgb: &[u8]) -> Result<CaptureData, Failure> {
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, u32::from(width), u32::from(height));
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|_| Failure::internal("cannot initialize PNG capture encoding"))?;
        writer
            .write_image_data(rgb)
            .map_err(|_| Failure::internal("cannot encode PNG capture data"))?;
    }
    if png.len() > MAX_CAPTURE_PNG_BYTES {
        return Err(Failure::too_large(
            "encoded capture exceeds the published image bound",
        ));
    }
    CaptureData::new(BASE64.encode(png))
        .map_err(|_| Failure::internal("base64 capture exceeded its derived bound"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn true_color_components_are_scaled_to_eight_bits() {
        assert_eq!(component(0xf800, 0xf800).ok(), Some(255));
        assert_eq!(component(0x0400, 0x07e0).ok(), Some(130));
        assert_eq!(component(0x000f, 0x001f).ok(), Some(123));
        assert!(component(0, 0).is_err());
    }

    #[test]
    fn pixel_byte_order_is_explicit() {
        assert_eq!(pixel_value(&[0x12, 0x34], ImageOrder::LSB_FIRST), 0x3412);
        assert_eq!(pixel_value(&[0x12, 0x34], ImageOrder::MSB_FIRST), 0x1234);
    }
}
