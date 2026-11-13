//! Guest Forms objects. These records do not create host windows.

use crate::vm::{Machine, Value};
use std::collections::BTreeMap;

#[derive(Default)]
pub struct Forms {
    bitmaps: BTreeMap<usize, pocket_kernel::Framebuffer>,
    images: Vec<Option<image::RgbaImage>>,
    controls: BTreeMap<usize, Control>,
    fonts: BTreeMap<usize, Font>,
    main_form: Option<usize>,
    paint: Option<u32>,
    pointer: (i32, i32),
    close_requested: bool,
    display_size: (i32, i32),
    invalidated: bool,
    next_tick: BTreeMap<usize, u64>,
}

struct Font {
    family: Value,
    size: u32,
}

#[derive(Default)]
struct Control {
    kind: &'static str,
    is_form: bool,
    image: Option<Value>,
    integer_properties: BTreeMap<&'static str, i32>,
    layout_suspensions: usize,
    back_color: Option<i32>,
    name: Option<u32>,
    font: Option<usize>,
    text: Option<u32>,
    children: Vec<usize>,
    items: Vec<Value>,
    handlers: BTreeMap<&'static str, Vec<Value>>,
    position: (i32, i32),
    size: (i32, i32),
    scale_dimensions: (u32, u32),
}

impl Forms {
    pub fn roots(&self, roots: &mut Vec<Value>) {
        roots.extend(self.main_form.map(Value::Reference));
        roots.extend(self.controls.iter().filter_map(|(&id, control)| {
            (control.handlers.contains_key("Tick")
                && control.integer_properties.get("Enabled") == Some(&1))
            .then_some(Value::Reference(id))
        }));
    }

    pub fn trace_control(&self, id: usize, roots: &mut Vec<Value>) {
        if let Some(control) = self.controls.get(&id) {
            roots.extend(control.children.iter().copied().map(Value::Reference));
            roots.extend(control.image);
            roots.extend(control.font.map(Value::Reference));
            roots.extend(control.items.iter().copied());
            roots.extend(control.handlers.values().flatten().copied());
        }
    }

    pub fn sweep(&mut self, live: &[bool], images: &std::collections::BTreeSet<usize>) {
        self.controls.retain(|&id, _| live[id]);
        self.next_tick.retain(|&id, _| live[id]);
        self.fonts.retain(|&id, _| live[id]);
        self.bitmaps.retain(|&id, _| live[id]);
        for (id, image) in self.images.iter_mut().enumerate() {
            if !images.contains(&id) {
                *image = None;
            }
        }
    }

    pub fn set_bitmap(&mut self, id: usize, w: u32, h: u32) {
        self.bitmaps
            .insert(id, pocket_kernel::Framebuffer::new(w, h));
    }

    pub fn bitmap_mut(&mut self, id: usize) -> Option<&mut pocket_kernel::Framebuffer> {
        self.bitmaps.get_mut(&id)
    }

    pub fn draw_resource(&mut self, target: usize, image: usize, x: i32, y: i32) -> Option<()> {
        let bitmap = self.bitmaps.get_mut(&target)?;
        let bounds = [0, 0, bitmap.width as i32, bitmap.height as i32];
        Self::draw_image(bitmap, self.images.get(image)?.as_ref()?, x, y, bounds);
        Some(())
    }

    fn draw_image(
        target: &mut pocket_kernel::Framebuffer,
        image: &image::RgbaImage,
        x: i32,
        y: i32,
        bounds: [i32; 4],
    ) {
        let [left, top, right, bottom] = bounds;
        for py in top.max(y)..bottom.min(y.saturating_add(image.height() as i32)) {
            for px in left.max(x)..right.min(x.saturating_add(image.width() as i32)) {
                let [r, g, b, a] = image.get_pixel((px - x) as u32, (py - y) as u32).0;
                if a != 0 {
                    let offset = (py as usize * target.width as usize + px as usize) * 2;
                    target.pixels[offset..offset + 2].copy_from_slice(
                        &pocket_kernel::framebuffer::pack_rgb565(r, g, b).to_le_bytes(),
                    );
                }
            }
        }
    }

    pub fn add_image(&mut self, image: image::RgbaImage) -> usize {
        if let Some(id) = self.images.iter().position(Option::is_none) {
            self.images[id] = Some(image);
            return id;
        }
        self.images.push(Some(image));
        self.images.len() - 1
    }

    pub fn run(&mut self, value: Value, paint: Option<u32>) -> Option<()> {
        let Value::Reference(id) = value else {
            return None;
        };
        self.controls.get(&id)?;
        self.main_form = Some(id);
        self.paint = paint;
        self.invalidated = true;
        Some(())
    }

    pub fn set_display_size(&mut self, size: (u32, u32)) {
        self.display_size = (size.0 as i32, size.1 as i32);
    }

    pub fn set_pointer(&mut self, x: u16, y: u16) {
        self.pointer = (i32::from(x), i32::from(y));
    }

    pub fn click_events(&mut self, x: u16, y: u16) -> Vec<(usize, Value, Value)> {
        let Some(root) = self.main_form else {
            return Vec::new();
        };
        let mut pending = vec![(root, (0i32, 0i32))];
        let mut seen = std::collections::BTreeSet::new();
        let mut hit = root;
        let mut position = (0, 0);
        let (x, y) = (i32::from(x), i32::from(y));
        while let Some((id, (left, top))) = pending.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some(control) = self.controls.get(&id) else {
                continue;
            };
            if control.integer_properties.get("Visible") == Some(&0)
                || control.integer_properties.get("Enabled") == Some(&0)
            {
                continue;
            }
            let left = left.saturating_add(control.position.0);
            let top = top.saturating_add(control.position.1);
            if id == root
                || (x >= left
                    && y >= top
                    && x < left.saturating_add(control.size.0)
                    && y < top.saturating_add(control.size.1))
            {
                hit = id;
                position = (left, top);
                pending.extend(control.children.iter().map(|id| (*id, (left, top))));
            }
        }
        let Some(control) = self.controls.get_mut(&hit) else {
            return Vec::new();
        };
        let changed = match control.kind {
            "CheckBox" => {
                let checked = control.integer_properties.entry("Checked").or_default();
                *checked = i32::from(*checked == 0);
                Some("CheckedChanged")
            }
            "ComboBox" if !control.items.is_empty() => {
                let selected = control
                    .integer_properties
                    .entry("SelectedIndex")
                    .or_insert(-1);
                *selected = (*selected + 1).rem_euclid(control.items.len() as i32);
                Some("SelectedIndexChanged")
            }
            _ => None,
        };
        self.invalidated = true;
        let mut events = Vec::new();
        if let Some(handlers) = changed
            .and_then(|event| control.handlers.get(event))
            .or_else(|| control.handlers.get("Click"))
        {
            events.extend(
                handlers
                    .iter()
                    .map(|handler| (hit, *handler, Value::EventArgs)),
            );
        }
        if let Some(handlers) = control.handlers.get("MouseUp") {
            let args =
                Value::MouseEvent(x.saturating_sub(position.0), y.saturating_sub(position.1));
            events.extend(handlers.iter().map(|handler| (hit, *handler, args)));
        }
        events
    }

    pub fn paint_controls(
        &self,
        framebuffer: &mut pocket_kernel::Framebuffer,
        mut string: impl FnMut(u32) -> Option<String>,
    ) {
        use pocket_kernel::framebuffer::pack_rgb565;
        let Some(root) = self.main_form.and_then(|id| self.controls.get(&id)) else {
            return;
        };
        let mut pending: Vec<_> = root
            .children
            .iter()
            .rev()
            .map(|id| (*id, 0i32, 0i32))
            .collect();
        let mut visited = std::collections::BTreeSet::new();
        while let Some((id, parent_x, parent_y)) = pending.pop() {
            if !visited.insert(id) {
                continue;
            }
            let Some(control) = self.controls.get(&id) else {
                continue;
            };
            if control.integer_properties.get("Visible") == Some(&0) {
                continue;
            }
            let x = parent_x.saturating_add(control.position.0);
            let y = parent_y.saturating_add(control.position.1);
            let (w, h) = control.size;
            let screen_w = framebuffer.width as i32;
            let screen_h = framebuffer.height as i32;
            let left = x.clamp(0, screen_w);
            let top = y.clamp(0, screen_h);
            let right = x.saturating_add(w).clamp(0, screen_w);
            let bottom = y.saturating_add(h).clamp(0, screen_h);
            if left < right && top < bottom {
                match control.image {
                    Some(Value::Reference(id)) => {
                        if let Some(bitmap) = self.bitmaps.get(&id) {
                            framebuffer.blit_from(
                                left,
                                top,
                                left - x,
                                top - y,
                                right - left,
                                bottom - top,
                                &bitmap.pixels,
                                bitmap.width,
                                bitmap.height,
                            );
                        }
                    }
                    Some(Value::Image(id)) => {
                        if let Some(image) = self.images.get(id).and_then(Option::as_ref) {
                            Self::draw_image(framebuffer, image, x, y, [left, top, right, bottom]);
                        }
                    }
                    _ => {}
                }
                let back = control
                    .back_color
                    .map(|color| pack_rgb565((color >> 16) as u8, (color >> 8) as u8, color as u8));
                let face = pack_rgb565(230, 230, 230);
                let outline = pack_rgb565(75, 75, 75);
                if let Some(color) =
                    back.or_else(|| matches!(control.kind, "Button" | "ComboBox").then_some(face))
                {
                    framebuffer.fill_rect(x, y, w, h, color);
                }
                if matches!(control.kind, "Button" | "ComboBox") {
                    framebuffer.stroke_rect(left, top, right - left, bottom - top, outline);
                }
                let mut text_x = x.saturating_add(3);
                if control.kind == "CheckBox" {
                    framebuffer.fill_rect(x, y, 12, 12, face);
                    framebuffer.stroke_rect(
                        left,
                        top,
                        x.saturating_add(12).min(right).saturating_sub(left),
                        y.saturating_add(12).min(bottom).saturating_sub(top),
                        outline,
                    );
                    if control.integer_properties.get("Checked") == Some(&1) {
                        framebuffer.fill_rect(
                            x.saturating_add(3),
                            y.saturating_add(3),
                            6,
                            6,
                            outline,
                        );
                    }
                    text_x = x.saturating_add(16);
                }
                let selected = control
                    .integer_properties
                    .get("SelectedIndex")
                    .copied()
                    .unwrap_or(-1);
                let selected = match control.items.get(selected as usize) {
                    Some(Value::StringToken(token)) => Some(*token),
                    _ => None,
                };
                if let Some(text) = selected.or(control.text).and_then(&mut string) {
                    let utf16: Vec<_> = text.encode_utf16().collect();
                    let font = control.font.and_then(|id| self.render_font(id));
                    let layout = pocket_kernel::text::layout(font.as_ref(), &utf16);
                    layout.draw(
                        &mut pocket_kernel::gdi::Surface::Screen(framebuffer),
                        text_x.max(-2048),
                        y.saturating_add(2).max(-2048),
                        0,
                        [left, top, right, bottom],
                    );
                }
            }
            pending.extend(control.children.iter().rev().map(|id| (*id, x, y)));
        }
    }

    pub fn request_close(&mut self) {
        self.close_requested = self.main_form.is_some();
    }

    pub fn close_events(&mut self) -> Option<Vec<(usize, Value)>> {
        if !std::mem::take(&mut self.close_requested) {
            return None;
        }
        let id = self.main_form.take()?;
        Some(
            self.controls
                .get(&id)?
                .handlers
                .get("Closing")
                .map_or_else(Vec::new, |handlers| {
                    handlers.iter().map(|value| (id, *value)).collect()
                }),
        )
    }

    pub fn canvas_size(&self) -> (u32, u32) {
        let (width, height) = self.display_size();
        let (w, h) = self
            .main_form
            .and_then(|id| self.controls.get(&id))
            .map_or((0, 0), |form| form.size);
        if (1..=2048).contains(&w)
            && (1..=2048).contains(&h)
            && (w as u32 > width || h as u32 > height)
        {
            (w as u32, h as u32)
        } else {
            (width, height)
        }
    }

    pub fn display_size(&self) -> (u32, u32) {
        (self.display_size.0 as u32, self.display_size.1 as u32)
    }

    pub fn take_invalidated(&mut self) -> bool {
        std::mem::take(&mut self.invalidated)
    }

    pub fn take_paint(&mut self) -> Option<(usize, u32)> {
        self.paint?;
        if !self.take_invalidated() {
            return None;
        }
        Some((self.main_form?, self.paint?))
    }

    pub fn running(&self) -> bool {
        self.main_form.is_some()
    }

    pub fn initial_events(&self) -> Vec<(usize, Value)> {
        self.main_form
            .and_then(|id| {
                self.controls
                    .get(&id)?
                    .handlers
                    .get("Load")
                    .map(|handlers| (id, handlers))
            })
            .map_or_else(Vec::new, |(id, handlers)| {
                handlers.iter().map(|handler| (id, *handler)).collect()
            })
    }

    pub fn timer_events(&mut self, now: u64) -> Vec<(usize, Value)> {
        let mut events = Vec::new();
        for (&id, control) in &self.controls {
            if control.integer_properties.get("Enabled") != Some(&1) {
                self.next_tick.remove(&id);
                continue;
            }
            let Some(handlers) = control.handlers.get("Tick") else {
                continue;
            };
            let interval = control
                .integer_properties
                .get("Interval")
                .copied()
                .unwrap_or(100)
                .max(1) as u64;
            let due = self
                .next_tick
                .entry(id)
                .or_insert_with(|| now.saturating_add(interval));
            if now >= *due {
                *due = now.saturating_add(interval);
                events.extend(handlers.iter().map(|handler| (id, *handler)));
            }
        }
        events
    }

    pub fn render_font(&self, id: usize) -> Option<pocket_kernel::gdi::Font> {
        let font = self.fonts.get(&id)?;
        Some(pocket_kernel::gdi::Font::new(
            -(f32::from_bits(font.size).clamp(6.0, 48.0) as i32),
        ))
    }

    pub fn font_family(&self, id: usize) -> Option<Value> {
        Some(self.fonts.get(&id)?.family)
    }

    pub fn font(&mut self, args: &[Value]) -> Option<()> {
        let [Value::Reference(id), family @ (Value::StringToken(_) | Value::HostString(_)), Value::Float(size), Value::Int(_style)] =
            args
        else {
            return None;
        };
        self.fonts.insert(
            *id,
            Font {
                family: *family,
                size: *size,
            },
        );
        Some(())
    }

    pub fn call(
        &mut self,
        machine: &Machine,
        kind: &str,
        method: &str,
        args: &[Value],
    ) -> Option<Option<Value>> {
        match (method, args) {
            (".ctor", [Value::Reference(id)]) => {
                let is_form = kind == "Form";
                let kind = match kind {
                    "CheckBox" => "CheckBox",
                    "ComboBox" => "ComboBox",
                    "Button" => "Button",
                    "Label" => "Label",
                    "PictureBox" => "PictureBox",
                    _ => "Container",
                };
                self.controls.insert(
                    *id,
                    Control {
                        kind,
                        is_form,
                        ..Control::default()
                    },
                );
                Some(None)
            }
            ("SuspendLayout", [Value::Reference(id)]) => {
                let control = self.controls.get_mut(id)?;
                control.layout_suspensions = control.layout_suspensions.checked_add(1)?;
                Some(None)
            }
            ("ResumeLayout", [Value::Reference(id)] | [Value::Reference(id), Value::Int(_)]) => {
                let control = self.controls.get_mut(id)?;
                control.layout_suspensions = control.layout_suspensions.saturating_sub(1);
                Some(None)
            }
            ("set_BackColor", [Value::Reference(id), Value::Int(color)]) => {
                self.controls.get_mut(id)?.back_color = Some(*color);
                Some(None)
            }
            ("set_Font", [Value::Reference(id), Value::Reference(font)]) => {
                self.fonts.get(font)?;
                self.controls.get_mut(id)?.font = Some(*font);
                Some(None)
            }
            ("set_Name", [Value::Reference(id), Value::StringToken(name)]) => {
                self.controls.get_mut(id)?.name = Some(*name);
                Some(None)
            }
            ("set_Text", [Value::Reference(id), Value::StringToken(text)]) => {
                self.controls.get_mut(id)?.text = Some(*text);
                Some(None)
            }
            ("get_Items", [Value::Reference(id)]) => {
                self.controls.get(id)?;
                Some(Some(Value::ItemCollection(*id)))
            }
            ("AddRange", [Value::ItemCollection(id), array]) => {
                self.controls
                    .get_mut(id)?
                    .items
                    .extend_from_slice(machine.array(*array)?);
                Some(None)
            }
            ("Add", [Value::ItemCollection(id), value]) => {
                let items = &mut self.controls.get_mut(id)?.items;
                let index = i32::try_from(items.len()).ok()?;
                items.push(*value);
                Some(Some(Value::Int(index)))
            }
            ("Close", [Value::Reference(id)]) if self.main_form == Some(*id) => {
                self.close_requested = true;
                Some(None)
            }
            ("get_Checked" | "get_SelectedIndex" | "get_Interval", [Value::Reference(id)]) => {
                let control = self.controls.get(id)?;
                let (property, default) = if method == "get_Checked" {
                    ("Checked", 0)
                } else if method == "get_Interval" {
                    ("Interval", 100)
                } else {
                    ("SelectedIndex", -1)
                };
                Some(Some(Value::Int(
                    control
                        .integer_properties
                        .get(property)
                        .copied()
                        .unwrap_or(default),
                )))
            }
            ("get_MousePosition", []) => Some(Some(Value::Point(self.pointer.0, self.pointer.1))),
            ("Invalidate", [Value::Reference(id)]) => {
                self.controls.get(id)?;
                if self.main_form == Some(*id) {
                    self.invalidated = true;
                }
                Some(None)
            }
            ("get_ClientRectangle", [Value::Reference(id)]) => {
                let control = self.controls.get(id)?;
                let (w, h) = if control.is_form
                    && control.integer_properties.get("WindowState") == Some(&2)
                {
                    self.display_size
                } else {
                    control.size
                };
                Some(Some(Value::Rectangle(0, 0, w, h)))
            }
            ("get_Width" | "get_Height", [Value::Reference(id)]) => {
                let (w, h) = self.controls.get(id)?.size;
                Some(Some(Value::Int(if method == "get_Width" { w } else { h })))
            }
            ("get_Handle", [Value::Reference(id)]) => {
                self.controls.get(id)?;
                Some(Some(Value::Int(i32::try_from(id.checked_add(1)?).ok()?)))
            }
            ("get_Controls", [Value::Reference(id)]) => {
                self.controls.get(id)?;
                Some(Some(Value::ControlCollection(*id)))
            }
            ("Remove", [Value::ControlCollection(parent), Value::Reference(child)]) => {
                self.controls
                    .get_mut(parent)?
                    .children
                    .retain(|id| id != child);
                self.invalidated = true;
                Some(None)
            }
            ("Clear", [Value::ControlCollection(parent)]) => {
                self.controls.get_mut(parent)?.children.clear();
                self.invalidated = true;
                Some(None)
            }
            ("Add", [Value::ControlCollection(parent), Value::Reference(child)]) => {
                if parent == child {
                    return None;
                }
                self.controls.get(child)?;
                let control = self.controls.get_mut(parent)?;
                if !control.children.contains(child) {
                    control.children.push(*child);
                }
                Some(None)
            }
            ("set_AutoScaleDimensions", [Value::Reference(id), Value::SizeF(w, h)]) => {
                self.controls.get_mut(id)?.scale_dimensions = (*w, *h);
                Some(None)
            }
            ("set_Image", [Value::Reference(id), image]) => {
                self.controls.get_mut(id)?.image = Some(*image);
                self.invalidated = true;
                Some(None)
            }
            ("get_Image", [Value::Reference(id)]) => {
                Some(Some(self.controls.get(id)?.image.unwrap_or(Value::Null)))
            }
            ("set_Icon", [Value::Reference(id), _]) => {
                self.controls.get(id)?;
                Some(None)
            }
            ("set_Location", [Value::Reference(id), Value::Point(x, y)]) => {
                self.controls.get_mut(id)?.position = (*x, *y);
                Some(None)
            }
            ("set_Size" | "set_ClientSize", [Value::Reference(id), Value::Size(w, h)]) => {
                self.controls.get_mut(id)?.size = (*w, *h);
                Some(None)
            }
            (method, [Value::Reference(id), handler @ Value::Delegate { .. }]) => {
                let (verb, name) = method.split_once('_')?;
                let event = match name {
                    "Click" => "Click",
                    "Tick" => "Tick",
                    "CheckedChanged" => "CheckedChanged",
                    "SelectedIndexChanged" => "SelectedIndexChanged",
                    "Load" => "Load",
                    "Deactivate" => "Deactivate",
                    "Closing" => "Closing",
                    "MouseUp" => "MouseUp",
                    _ => return None,
                };
                let handlers = self
                    .controls
                    .get_mut(id)?
                    .handlers
                    .entry(event)
                    .or_default();
                match verb {
                    "add" => handlers.push(*handler),
                    "remove" => {
                        if let Some(index) =
                            handlers.iter().rposition(|existing| existing == handler)
                        {
                            handlers.remove(index);
                        }
                    }
                    _ => return None,
                }
                Some(None)
            }
            (method, [Value::Reference(id), Value::Int(value)]) => {
                let property = match method {
                    "set_AutoScroll" => "AutoScroll",
                    "set_TabIndex" => "TabIndex",
                    "set_SelectedIndex" => "SelectedIndex",
                    "set_Checked" => "Checked",
                    "set_Visible" => "Visible",
                    "set_TextAlign" => "TextAlign",
                    "set_AutoScaleMode" => "AutoScaleMode",
                    "set_WindowState" => "WindowState",
                    "set_Enabled" => "Enabled",
                    "set_Interval" if *value > 0 => "Interval",
                    _ => return None,
                };
                self.controls
                    .get_mut(id)?
                    .integer_properties
                    .insert(property, *value);
                Some(None)
            }
            _ => None,
        }
    }
}
