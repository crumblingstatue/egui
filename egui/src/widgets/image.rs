use crate::*;

#[derive(Clone, Copy, Debug)]
pub struct Image {
    texture_id: TextureId,
    uv: Rect,
    desired_size: Vec2,
    bg_fill: Srgba,
    tint: Srgba,
}

impl Image {
    pub fn new(texture_id: TextureId, desired_size: Vec2) -> Self {
        Self {
            texture_id,
            uv: Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            desired_size,
            bg_fill: Default::default(),
            tint: color::WHITE,
        }
    }

    /// Select UV range. Default is (0,0) in top-left, (1,1) bottom right.
    pub fn uv(mut self, uv: impl Into<Rect>) -> Self {
        self.uv = uv.into();
        self
    }

    /// A solid color to put behind the image. Useful for transparent images.
    pub fn bg_fill(mut self, bg_fill: impl Into<Srgba>) -> Self {
        self.bg_fill = bg_fill.into();
        self
    }

    /// Multiply image color with this. Default is WHITE (no tint).
    pub fn tint(mut self, tint: impl Into<Srgba>) -> Self {
        self.tint = tint.into();
        self
    }
}

impl Widget for Image {
    fn ui(self, ui: &mut Ui) -> Response {
        use paint::*;
        let Self {
            texture_id,
            uv,
            desired_size,
            bg_fill,
            tint,
        } = self;
        let rect = ui.allocate_space(desired_size);
        if bg_fill != Default::default() {
            let mut triangles = Triangles::default();
            triangles.add_colored_rect(rect, bg_fill);
            ui.painter().add(PaintCmd::triangles(triangles));
        }
        {
            // TODO: builder pattern for Triangles
            let mut triangles = Triangles::with_texture(texture_id);
            triangles.add_rect_with_uv(rect, uv, tint);
            ui.painter().add(PaintCmd::triangles(triangles));
        }

        ui.interact_hover(rect)
    }
}
