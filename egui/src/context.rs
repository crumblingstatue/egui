use std::sync::{
    atomic::{AtomicU32, Ordering::SeqCst},
    Arc,
};

use ahash::AHashMap;

use crate::{
    animation_manager::AnimationManager,
    mutex::{Mutex, MutexGuard},
    paint::{stats::*, *},
    *,
};

#[derive(Clone, Copy, Default)]
struct SliceStats<T>(usize, std::marker::PhantomData<T>);

#[derive(Clone, Debug, Default)]
struct Options {
    /// The default style for new `Ui`:s.
    style: Arc<Style>,
    /// Controls the tessellator.
    paint_options: paint::PaintOptions,
    /// Font sizes etc.
    font_definitions: FontDefinitions,
}

/// Thi is the first thing you need when working with Egui.
///
/// Contains the input state, memory, options and output.
/// `Ui`:s keep an `Arc` pointer to this.
/// This allows us to create several child `Ui`:s at once,
/// all working against the same shared Context.
// TODO: too many mutexes. Maybe put it all behind one Mutex instead.
#[derive(Default)]
pub struct Context {
    options: Mutex<Options>,
    /// None until first call to `begin_frame`.
    fonts: Option<Arc<Fonts>>,
    memory: Arc<Mutex<Memory>>,
    animation_manager: Arc<Mutex<AnimationManager>>,

    input: InputState,

    /// Starts off as the screen_rect, shrinks as panels are added.
    /// Becomes `Rect::nothing()` after a `CentralPanel` is finished.
    available_rect: Mutex<Option<Rect>>,
    /// How much space is used by panels.
    used_by_panels: Mutex<Option<Rect>>,

    // The output of a frame:
    graphics: Mutex<GraphicLayers>,
    output: Mutex<Output>,
    /// Used to debug name clashes of e.g. windows
    used_ids: Mutex<AHashMap<Id, Pos2>>,

    paint_stats: Mutex<PaintStats>,

    /// While positive, keep requesting repaints. Decrement at the end of each frame.
    repaint_requests: AtomicU32,
}

impl Clone for Context {
    fn clone(&self) -> Self {
        Context {
            options: self.options.clone(),
            fonts: self.fonts.clone(),
            memory: self.memory.clone(),
            animation_manager: self.animation_manager.clone(),
            input: self.input.clone(),
            available_rect: self.available_rect.clone(),
            used_by_panels: self.used_by_panels.clone(),
            graphics: self.graphics.clone(),
            output: self.output.clone(),
            used_ids: self.used_ids.clone(),
            paint_stats: self.paint_stats.clone(),
            repaint_requests: self.repaint_requests.load(SeqCst).into(),
        }
    }
}

impl Context {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// How much space is still available after panels has been added.
    /// This is the "background" area, what Egui doesn't cover with panels (but may cover with windows).
    /// This is also the area to which windows are constrained.
    pub fn available_rect(&self) -> Rect {
        self.available_rect
            .lock()
            .expect("Called `available_rect()` before `begin_frame()`")
    }

    pub fn memory(&self) -> MutexGuard<'_, Memory> {
        self.memory.lock()
    }

    pub fn graphics(&self) -> MutexGuard<'_, GraphicLayers> {
        self.graphics.lock()
    }

    pub fn output(&self) -> MutexGuard<'_, Output> {
        self.output.lock()
    }

    /// Call this if there is need to repaint the UI, i.e. if you are showing an animation.
    /// If this is called at least once in a frame, then there will be another frame right after this.
    /// Call as many times as you wish, only one repaint will be issued.
    pub fn request_repaint(&self) {
        // request two frames of repaint, just to cover some corner cases (frame delays):
        let times_to_repaint = 2;
        self.repaint_requests.store(times_to_repaint, SeqCst);
    }

    pub fn input(&self) -> &InputState {
        &self.input
    }

    /// Not valid until first call to `begin_frame()`
    /// That's because since we don't know the proper `pixels_per_point` until then.
    pub fn fonts(&self) -> &Fonts {
        &*self
            .fonts
            .as_ref()
            .expect("No fonts available until first call to Context::begin_frame()`")
    }

    /// The Egui texture, containing font characters etc..
    /// Not valid until first call to `begin_frame()`
    /// That's because since we don't know the proper `pixels_per_point` until then.
    pub fn texture(&self) -> Arc<paint::Texture> {
        self.fonts().texture()
    }

    /// Will become active at the start of the next frame.
    /// `pixels_per_point` will be ignored (overwritten at start of each frame with the contents of input)
    pub fn set_fonts(&self, font_definitions: FontDefinitions) {
        self.options.lock().font_definitions = font_definitions;
    }

    pub fn style(&self) -> Arc<Style> {
        self.options.lock().style.clone()
    }

    pub fn set_style(&self, style: impl Into<Arc<Style>>) {
        self.options.lock().style = style.into();
    }

    pub fn pixels_per_point(&self) -> f32 {
        self.input.pixels_per_point()
    }

    /// Useful for pixel-perfect rendering
    pub fn round_to_pixel(&self, point: f32) -> f32 {
        let pixels_per_point = self.pixels_per_point();
        (point * pixels_per_point).round() / pixels_per_point
    }

    /// Useful for pixel-perfect rendering
    pub fn round_pos_to_pixels(&self, pos: Pos2) -> Pos2 {
        pos2(self.round_to_pixel(pos.x), self.round_to_pixel(pos.y))
    }

    /// Useful for pixel-perfect rendering
    pub fn round_vec_to_pixels(&self, vec: Vec2) -> Vec2 {
        vec2(self.round_to_pixel(vec.x), self.round_to_pixel(vec.y))
    }

    /// Useful for pixel-perfect rendering
    pub fn round_rect_to_pixels(&self, rect: Rect) -> Rect {
        Rect {
            min: self.round_pos_to_pixels(rect.min),
            max: self.round_pos_to_pixels(rect.max),
        }
    }

    // ---------------------------------------------------------------------

    /// Constraint the position of a window/area
    /// so it fits within the screen.
    pub(crate) fn constrain_window_rect(&self, window: Rect) -> Rect {
        let screen = self.available_rect();

        let mut pos = window.min;

        // Constrain to screen, unless window is too large to fit:
        let margin_x = (window.width() - screen.width()).at_least(0.0);
        let margin_y = (window.height() - screen.height()).at_least(0.0);

        pos.x = pos.x.at_least(screen.left() - margin_x);
        pos.x = pos.x.at_most(screen.right() + margin_x - window.width());
        pos.y = pos.y.at_least(screen.top() - margin_y);
        pos.y = pos.y.at_most(screen.bottom() + margin_y - window.height());

        pos = self.round_pos_to_pixels(pos);

        Rect::from_min_size(pos, window.size())
    }

    // ---------------------------------------------------------------------

    /// Call at the start of every frame.
    /// Put your widgets into a `SidePanel`, `TopPanel`, `CentralPanel`, `Window` or `Area`.
    pub fn begin_frame(self: &mut Arc<Self>, new_input: RawInput) {
        let mut self_: Self = (**self).clone();
        self_.begin_frame_mut(new_input);
        *self = Arc::new(self_);
    }

    fn begin_frame_mut(&mut self, new_raw_input: RawInput) {
        self.memory().begin_frame(&self.input);

        self.used_ids.lock().clear();

        self.input = std::mem::take(&mut self.input).begin_frame(new_raw_input);
        *self.available_rect.lock() = Some(self.input.screen_rect());
        *self.used_by_panels.lock() = Some(Rect::nothing());

        let mut font_definitions = self.options.lock().font_definitions.clone();
        font_definitions.pixels_per_point = self.input.pixels_per_point();
        let same_as_current = match &self.fonts {
            None => false,
            Some(fonts) => *fonts.definitions() == font_definitions,
        };
        if !same_as_current {
            self.fonts = Some(Arc::new(Fonts::from_definitions(font_definitions)));
        }

        // Ensure we register the background area so panels and background ui can catch clicks:
        let screen_rect = self.input.screen_rect();
        self.memory().areas.set_state(
            LayerId::background(),
            containers::area::State {
                pos: screen_rect.min,
                size: screen_rect.size(),
                interactable: true,
            },
        );
    }

    /// Call at the end of each frame.
    /// Returns what has happened this frame (`Output`) as well as what you need to paint.
    #[must_use]
    pub fn end_frame(&self) -> (Output, PaintJobs) {
        if self.input.wants_repaint() {
            self.request_repaint();
        }

        self.memory().end_frame();

        let mut output: Output = std::mem::take(&mut self.output());
        if self.repaint_requests.load(SeqCst) > 0 {
            self.repaint_requests.fetch_sub(1, SeqCst);
            output.needs_repaint = true;
        }

        let paint_jobs = self.paint();
        (output, paint_jobs)
    }

    fn drain_paint_lists(&self) -> Vec<(Rect, PaintCmd)> {
        let memory = self.memory();
        self.graphics().drain(memory.areas.order()).collect()
    }

    fn paint(&self) -> PaintJobs {
        let mut paint_options = self.options.lock().paint_options;
        paint_options.aa_size = 1.0 / self.pixels_per_point();
        let paint_commands = self.drain_paint_lists();
        let paint_stats = PaintStats::from_paint_commands(&paint_commands); // TODO: internal allocations
        let paint_jobs =
            tessellator::tessellate_paint_commands(paint_commands, paint_options, self.fonts());
        *self.paint_stats.lock() = paint_stats.with_paint_jobs(&paint_jobs);

        paint_jobs
    }

    // ---------------------------------------------------------------------

    /// Shrink `available_rect()`.
    pub(crate) fn allocate_left_panel(&self, panel_rect: Rect) {
        let mut remainder = self.available_rect();
        remainder.min.x = panel_rect.max.x;
        *self.available_rect.lock() = Some(remainder);
        self.register_panel(panel_rect);
    }

    /// Shrink `available_rect()`.
    pub(crate) fn allocate_top_panel(&self, panel_rect: Rect) {
        let mut remainder = self.available_rect();
        remainder.min.y = panel_rect.max.y;
        *self.available_rect.lock() = Some(remainder);
        self.register_panel(panel_rect);
    }

    /// Shrink `available_rect()`.
    pub(crate) fn allocate_central_panel(&self, panel_rect: Rect) {
        let mut available_rect = self.available_rect.lock();
        debug_assert!(
            *available_rect != Some(Rect::nothing()),
            "You already created a  `CentralPanel` this frame!"
        );
        *available_rect = Some(Rect::nothing()); // Nothing left after this
        self.register_panel(panel_rect);
    }

    fn register_panel(&self, panel_rect: Rect) {
        let mut used = self.used_by_panels.lock();
        *used = Some(used.unwrap_or(Rect::nothing()).union(panel_rect));
    }

    /// How much space is used by panels and windows.
    pub fn used_rect(&self) -> Rect {
        let mut used = self.used_by_panels.lock().unwrap_or(Rect::nothing());
        for window in self.memory().areas.visible_windows() {
            used = used.union(window.rect());
        }
        used
    }

    /// How much space is used by panels and windows.
    /// You can shrink your Egui area to this size and still fit all Egui components.
    pub fn used_size(&self) -> Vec2 {
        self.used_rect().max - Pos2::new(0.0, 0.0)
    }

    // ---------------------------------------------------------------------

    /// Generate a id from the given source.
    /// If it is not unique, an error will be printed at the given position.
    pub fn make_unique_id<IdSource>(self: &Arc<Self>, source: IdSource, pos: Pos2) -> Id
    where
        IdSource: std::hash::Hash + std::fmt::Debug + Copy,
    {
        self.register_unique_id(Id::new(source), source, pos)
    }

    pub fn is_unique_id(&self, id: Id) -> bool {
        !self.used_ids.lock().contains_key(&id)
    }

    /// If the given Id is not unique, an error will be printed at the given position.
    pub fn register_unique_id(
        self: &Arc<Self>,
        id: Id,
        source_name: impl std::fmt::Debug,
        pos: Pos2,
    ) -> Id {
        if let Some(clash_pos) = self.used_ids.lock().insert(id, pos) {
            let painter = self.debug_painter();
            if clash_pos.distance(pos) < 4.0 {
                painter.error(
                    pos,
                    &format!("use of non-unique ID {:?} (name clash?)", source_name),
                );
            } else {
                painter.error(
                    clash_pos,
                    &format!("first use of non-unique ID {:?} (name clash?)", source_name),
                );
                painter.error(
                    pos,
                    &format!(
                        "second use of non-unique ID {:?} (name clash?)",
                        source_name
                    ),
                );
            }
            id
        } else {
            id
        }
    }

    // ---------------------------------------------------------------------

    /// Is the mouse over any Egui area?
    pub fn is_mouse_over_area(&self) -> bool {
        if let Some(mouse_pos) = self.input.mouse.pos {
            if let Some(layer) = self.layer_id_at(mouse_pos) {
                if layer.order == Order::Background {
                    if let Some(available_rect) = *self.available_rect.lock() {
                        // "available_rect" is the area that Egui is NOT using.
                        !available_rect.contains(mouse_pos)
                    } else {
                        false
                    }
                } else {
                    true
                }
            } else {
                false
            }
        } else {
            false
        }
    }

    /// True if Egui is currently interested in the mouse.
    /// Could be the mouse is hovering over a Egui window,
    /// or the user is dragging an Egui widget.
    /// If false, the mouse is outside of any Egui area and so
    /// you may be interested in what it is doing (e.g. controlling your game).
    /// Returns `false` if a drag starts outside of Egui and then moves over an Egui window.
    pub fn wants_mouse_input(&self) -> bool {
        self.is_using_mouse() || (self.is_mouse_over_area() && !self.input().mouse.down)
    }

    /// Is Egui currently using the mouse position (e.g. dragging a slider).
    /// NOTE: this will return false if the mouse is just hovering over an Egui window.
    pub fn is_using_mouse(&self) -> bool {
        self.memory().interaction.is_using_mouse()
    }

    /// If true, Egui is currently listening on text input (e.g. typing text in a `TextEdit`).
    pub fn wants_keyboard_input(&self) -> bool {
        self.memory().interaction.kb_focus_id.is_some()
    }

    // ---------------------------------------------------------------------

    pub fn layer_id_at(&self, pos: Pos2) -> Option<LayerId> {
        let resize_grab_radius_side = self.style().interaction.resize_grab_radius_side;
        self.memory().layer_id_at(pos, resize_grab_radius_side)
    }

    pub fn contains_mouse(&self, layer_id: LayerId, clip_rect: Rect, rect: Rect) -> bool {
        let rect = rect.intersect(clip_rect);
        if let Some(mouse_pos) = self.input.mouse.pos {
            rect.contains(mouse_pos) && self.layer_id_at(mouse_pos) == Some(layer_id)
        } else {
            false
        }
    }

    /// Use `ui.interact` instead
    pub(crate) fn interact(
        self: &Arc<Self>,
        layer_id: LayerId,
        clip_rect: Rect,
        rect: Rect,
        interaction_id: Option<Id>,
        sense: Sense,
    ) -> Response {
        let interact_rect = rect.expand2(0.5 * self.style().spacing.item_spacing); // make it easier to click. TODO: nice way to do this
        let hovered = self.contains_mouse(layer_id, clip_rect, interact_rect);
        let has_kb_focus = interaction_id
            .map(|id| self.memory().has_kb_focus(id))
            .unwrap_or(false);

        if interaction_id.is_none() || sense == Sense::nothing() {
            // Not interested in input:
            return Response {
                ctx: self.clone(),
                sense,
                rect,
                hovered,
                clicked: false,
                double_clicked: false,
                active: false,
                has_kb_focus,
            };
        }
        let interaction_id = interaction_id.unwrap();

        let mut memory = self.memory();

        memory.interaction.click_interest |= hovered && sense.click;
        memory.interaction.drag_interest |= hovered && sense.drag;

        let active = memory.interaction.click_id == Some(interaction_id)
            || memory.interaction.drag_id == Some(interaction_id);

        if self.input.mouse.pressed {
            if hovered {
                let mut response = Response {
                    ctx: self.clone(),
                    sense,
                    rect,
                    hovered: true,
                    clicked: false,
                    double_clicked: false,
                    active: false,
                    has_kb_focus,
                };

                if sense.click && memory.interaction.click_id.is_none() {
                    // start of a click
                    memory.interaction.click_id = Some(interaction_id);
                    response.active = true;
                }

                if sense.drag
                    && (memory.interaction.drag_id.is_none() || memory.interaction.drag_is_window)
                {
                    // start of a drag
                    memory.interaction.drag_id = Some(interaction_id);
                    memory.interaction.drag_is_window = false;
                    memory.window_interaction = None; // HACK: stop moving windows (if any)
                    response.active = true;
                }

                response
            } else {
                // miss
                Response {
                    ctx: self.clone(),
                    sense,
                    rect,
                    hovered,
                    clicked: false,
                    double_clicked: false,
                    active: false,
                    has_kb_focus,
                }
            }
        } else if self.input.mouse.released {
            let clicked = hovered && active && self.input.mouse.could_be_click;
            Response {
                ctx: self.clone(),
                sense,
                rect,
                hovered,
                clicked,
                double_clicked: clicked && self.input.mouse.double_click,
                active,
                has_kb_focus,
            }
        } else if self.input.mouse.down {
            Response {
                ctx: self.clone(),
                sense,
                rect,
                hovered: hovered && active,
                clicked: false,
                double_clicked: false,
                active,
                has_kb_focus,
            }
        } else {
            Response {
                ctx: self.clone(),
                sense,
                rect,
                hovered,
                clicked: false,
                double_clicked: false,
                active,
                has_kb_focus,
            }
        }
    }
}

/// ## Animation
impl Context {
    /// Returns a value in the range [0, 1], to indicate "how on" this thing is.
    ///
    /// The first time called it will return `if value { 1.0 } else { 0.0 }`
    /// Calling this with `value = true` will always yield a number larger than zero, quickly going towards one.
    /// Calling this with `value = false` will always yield a number less than one, quickly going towards zero.
    ///
    /// The function will call `request_repaint()` when appropriate.
    pub fn animate_bool(&self, id: Id, value: bool) -> f32 {
        let animation_time = self.style().animation_time;
        let animated_value =
            self.animation_manager
                .lock()
                .animate_bool(&self.input, animation_time, id, value);
        let animation_in_progress = 0.0 < animated_value && animated_value < 1.0;
        if animation_in_progress {
            self.request_repaint();
        }
        animated_value
    }
}

/// ## Painting
impl Context {
    pub fn debug_painter(self: &Arc<Self>) -> Painter {
        Painter::new(self.clone(), LayerId::debug(), self.input.screen_rect())
    }
}

impl Context {
    pub fn settings_ui(&self, ui: &mut Ui) {
        use crate::containers::*;

        CollapsingHeader::new("Style")
            .default_open(true)
            .show(ui, |ui| {
                self.style_ui(ui);
            });

        CollapsingHeader::new("Fonts")
            .default_open(false)
            .show(ui, |ui| {
                let mut font_definitions = self.fonts().definitions().clone();
                font_definitions.ui(ui);
                self.fonts().texture().ui(ui);
                self.set_fonts(font_definitions);
            });

        CollapsingHeader::new("Painting")
            .default_open(true)
            .show(ui, |ui| {
                let mut paint_options = self.options.lock().paint_options;
                paint_options.ui(ui);
                self.options.lock().paint_options = paint_options;
            });
    }

    pub fn inspection_ui(&self, ui: &mut Ui) {
        use crate::containers::*;

        ui.label(format!("Is using mouse: {}", self.is_using_mouse()))
            .on_hover_text("Is Egui currently using the mouse actively (e.g. dragging a slider)?");
        ui.label(format!("Wants mouse input: {}", self.wants_mouse_input()))
            .on_hover_text("Is Egui currently interested in the location of the mouse (either because it is in use, or because it is hovering over a window).");
        ui.label(format!(
            "Wants keyboard input: {}",
            self.wants_keyboard_input()
        ))
        .on_hover_text("Is Egui currently listening for text input");
        ui.advance_cursor(16.0);

        CollapsingHeader::new("Input")
            .default_open(false)
            .show(ui, |ui| ui.input().clone().ui(ui));

        CollapsingHeader::new("Paint stats")
            .default_open(true)
            .show(ui, |ui| {
                self.paint_stats.lock().ui(ui);
            });
    }

    pub fn memory_ui(&self, ui: &mut crate::Ui) {
        if ui
            .button("Reset all")
            .on_hover_text("Reset all Egui state")
            .clicked
        {
            *self.memory() = Default::default();
        }

        ui.horizontal(|ui| {
            ui.label(format!(
                "{} areas (window positions)",
                self.memory().areas.count()
            ));
            if ui.button("Reset").clicked {
                self.memory().areas = Default::default();
            }
        });
        ui.indent("areas", |ui| {
            let layers_ids: Vec<LayerId> = self.memory().areas.order().to_vec();
            for layer_id in layers_ids {
                let area = self.memory().areas.get(layer_id.id).cloned();
                if let Some(area) = area {
                    let is_visible = self.memory().areas.is_visible(&layer_id);
                    if ui
                        .label(format!(
                            "{:?} {:?} {}",
                            layer_id.order,
                            area.rect(),
                            if is_visible { "" } else { "(INVISIBLE)" }
                        ))
                        .hovered
                        && is_visible
                    {
                        ui.ctx()
                            .debug_painter()
                            .debug_rect(area.rect(), color::RED, "");
                    }
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label(format!(
                "{} collapsing headers",
                self.memory().collapsing_headers.len()
            ));
            if ui.button("Reset").clicked {
                self.memory().collapsing_headers = Default::default();
            }
        });

        ui.horizontal(|ui| {
            ui.label(format!("{} menu bars", self.memory().menu_bar.len()));
            if ui.button("Reset").clicked {
                self.memory().menu_bar = Default::default();
            }
        });

        ui.horizontal(|ui| {
            ui.label(format!("{} scroll areas", self.memory().scroll_areas.len()));
            if ui.button("Reset").clicked {
                self.memory().scroll_areas = Default::default();
            }
        });

        ui.horizontal(|ui| {
            ui.label(format!("{} resize areas", self.memory().resize.len()));
            if ui.button("Reset").clicked {
                self.memory().resize = Default::default();
            }
        });

        ui.shrink_width_to_current(); // don't let the text below grow this window wider
        ui.label("NOTE: the position of this window cannot be reset from within itself.");
    }
}

impl Context {
    pub fn style_ui(&self, ui: &mut Ui) {
        let mut style: Style = (*self.style()).clone();
        style.ui(ui);
        self.set_style(style);
    }
}

impl paint::PaintOptions {
    pub fn ui(&mut self, ui: &mut Ui) {
        let Self {
            aa_size: _,
            anti_alias,
            coarse_tessellation_culling,
            debug_paint_clip_rects,
            debug_ignore_clip_rects,
        } = self;
        ui.checkbox(anti_alias, "Antialias");
        ui.checkbox(
            coarse_tessellation_culling,
            "Do coarse culling in the tessellator",
        );
        ui.checkbox(debug_paint_clip_rects, "Paint clip rectangles (debug)");
        ui.checkbox(debug_ignore_clip_rects, "Ignore clip rectangles (debug)");
    }
}
