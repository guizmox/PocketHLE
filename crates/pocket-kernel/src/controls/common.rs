//! Common-control state and layout, independent of guest pointers/callbacks.
//! CERF SampleApp creates these during WM_CREATE; they must never reuse its proc.
use super::*;
use std::collections::BTreeMap;

pub const TREE_ROOT: u32 = 0xffff_0000;
pub const TREE_FIRST: u32 = 0xffff_0001;
pub const TREE_LAST: u32 = 0xffff_0002;
pub const TREE_SORT: u32 = 0xffff_0003;

#[derive(Debug, Clone, Default)]
pub struct TreeItem {
    pub handle: u32,
    pub parent: u32,
    pub children: Vec<u32>,
    pub text: String,
    pub state: u32,
    pub image: i32,
    pub selected_image: i32,
    pub children_hint: i32,
    pub data: u32,
    pub text_callback: bool,
    pub image_callback: bool,
}

#[derive(Debug, Clone)]
pub struct TreeState {
    pub items: BTreeMap<u32, TreeItem>,
    pub roots: Vec<u32>,
    pub selected: u32,
    pub drop_target: u32,
    pub first_visible: usize,
    pub indent: i32,
    pub row_height: i32,
    pub image_lists: [u32; 3],
    pub background: u32,
    pub text_color: u32,
    next_item: u32,
}
impl Default for TreeState {
    fn default() -> Self {
        Self {
            items: BTreeMap::new(),
            roots: Vec::new(),
            selected: 0,
            drop_target: 0,
            first_visible: 0,
            indent: 19,
            row_height: 20,
            image_lists: [0; 3],
            background: 0x00ff_ffff,
            text_color: 0,
            next_item: 1,
        }
    }
}
impl TreeState {
    pub fn insert(&mut self, parent: u32, after: u32, mut item: TreeItem) -> u32 {
        let parent = if parent == TREE_ROOT { 0 } else { parent };
        if self.items.len() >= 4096 || parent != 0 && !self.items.contains_key(&parent) {
            return 0;
        }
        let siblings = if parent == 0 {
            &self.roots
        } else {
            &self.items[&parent].children
        };
        let index = match after {
            TREE_FIRST => 0,
            TREE_LAST | 0 => siblings.len(),
            TREE_SORT => siblings
                .iter()
                .position(|h| self.items[h].text.to_lowercase() > item.text.to_lowercase())
                .unwrap_or(siblings.len()),
            _ => match siblings.iter().position(|h| *h == after) {
                Some(index) => index + 1,
                None => return 0,
            },
        };
        let Some(next) = self.next_item.checked_add(1).filter(|h| *h < TREE_ROOT) else {
            return 0;
        };
        let handle = self.next_item;
        self.next_item = next;
        item.handle = handle;
        item.parent = parent;
        self.items.insert(handle, item);
        if parent == 0 {
            self.roots.insert(index, handle);
        } else {
            self.items
                .get_mut(&parent)
                .unwrap()
                .children
                .insert(index, handle);
        }
        handle
    }
    pub fn remove(&mut self, handle: u32) -> bool {
        if handle == TREE_ROOT {
            self.items.clear();
            self.roots.clear();
            self.selected = 0;
            self.drop_target = 0;
            self.first_visible = 0;
            return true;
        }
        let Some(item) = self.items.get(&handle) else {
            return false;
        };
        let parent = item.parent;
        if parent == 0 {
            self.roots.retain(|h| *h != handle);
        } else if let Some(p) = self.items.get_mut(&parent) {
            p.children.retain(|h| *h != handle);
        }
        let mut pending = vec![handle];
        while let Some(h) = pending.pop() {
            if let Some(item) = self.items.remove(&h) {
                pending.extend(item.children);
            }
            if self.selected == h {
                self.selected = 0;
            }
            if self.drop_target == h {
                self.drop_target = 0;
            }
        }
        self.first_visible = self
            .first_visible
            .min(self.visible().len().saturating_sub(1));
        true
    }
    pub fn visible(&self) -> Vec<(u32, i32)> {
        let mut rows = Vec::new();
        let mut pending: Vec<_> = self.roots.iter().rev().map(|h| (*h, 0)).collect();
        while let Some((handle, depth)) = pending.pop() {
            let Some(item) = self.items.get(&handle) else {
                continue;
            };
            rows.push((handle, depth));
            if item.state & 0x20 != 0 {
                pending.extend(item.children.iter().rev().map(|h| (*h, depth + 1)));
            }
        }
        rows
    }
    pub fn sort(&mut self, parent: u32, recursive: bool) -> bool {
        let parent = if parent == TREE_ROOT { 0 } else { parent };
        if parent != 0 && !self.items.contains_key(&parent) {
            return false;
        }
        let mut pending = vec![parent];
        while let Some(h) = pending.pop() {
            let mut children = if h == 0 {
                self.roots.clone()
            } else {
                self.items[&h].children.clone()
            };
            children.sort_by_key(|h| self.items[h].text.to_lowercase());
            if recursive {
                pending.extend(children.iter().copied());
            }
            if h == 0 {
                self.roots = children;
            } else {
                self.items.get_mut(&h).unwrap().children = children;
            }
        }
        true
    }
    pub fn ensure_visible(&mut self, handle: u32, height: i32) -> bool {
        let Some(item) = self.items.get(&handle) else {
            return false;
        };
        let mut parent = item.parent;
        while let Some(p) = self.items.get_mut(&parent) {
            p.state |= 0x20;
            parent = p.parent;
        }
        let rows = self.visible();
        if let Some(index) = rows.iter().position(|(h, _)| *h == handle) {
            let count = (height / self.row_height.max(1)).max(1) as usize;
            if index < self.first_visible {
                self.first_visible = index;
            } else if index >= self.first_visible + count {
                self.first_visible = index + 1 - count;
            }
        }
        true
    }
    pub fn item_state(&self, item: &TreeItem) -> u32 {
        (item.state & !0x0a)
            | if self.selected == item.handle { 2 } else { 0 }
            | if self.drop_target == item.handle {
                8
            } else {
                0
            }
    }
    pub fn next(&self, handle: u32, relation: u32) -> u32 {
        match relation {
            0 => self.roots.first().copied().unwrap_or(0),
            3 => self.items.get(&handle).map_or(0, |i| i.parent),
            4 => self
                .items
                .get(&handle)
                .and_then(|i| i.children.first())
                .copied()
                .unwrap_or(0),
            5 => self
                .visible()
                .get(self.first_visible)
                .map_or(0, |(h, _)| *h),
            8 => self.drop_target,
            9 => self.selected,
            1 | 2 => {
                let Some(item) = self.items.get(&handle) else {
                    return 0;
                };
                let siblings = if item.parent == 0 {
                    &self.roots
                } else {
                    &self.items[&item.parent].children
                };
                let Some(index) = siblings.iter().position(|h| *h == handle) else {
                    return 0;
                };
                let next = if relation == 1 {
                    index.checked_add(1)
                } else {
                    index.checked_sub(1)
                };
                next.and_then(|n| siblings.get(n)).copied().unwrap_or(0)
            }
            6 | 7 => {
                let rows = self.visible();
                let Some(index) = rows.iter().position(|(h, _)| *h == handle) else {
                    return 0;
                };
                let next = if relation == 6 {
                    index.checked_add(1)
                } else {
                    index.checked_sub(1)
                };
                next.and_then(|n| rows.get(n)).map_or(0, |(h, _)| *h)
            }
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct HeaderItem {
    pub text: String,
    pub width: i32,
    pub format: u32,
    pub data: u32,
    pub bitmap: u32,
    pub image: i32,
}
#[derive(Debug, Clone, Default)]
pub struct HeaderState {
    pub items: Vec<HeaderItem>,
    pub order: Vec<usize>,
    pub image_list: u32,
}
impl HeaderState {
    pub fn rect(&self, index: usize, height: i32) -> Option<(i32, i32, i32, i32)> {
        let mut left: i32 = 0;
        for &i in &self.order {
            let item = self.items.get(i)?;
            if i == index {
                return Some((left, 0, item.width, height));
            }
            left = left.saturating_add(item.width);
        }
        None
    }
    pub fn hit(&self, x: i32, y: i32, height: i32) -> Option<usize> {
        self.order.iter().copied().find(|i| {
            self.rect(*i, height).is_some_and(|(left, _, w, h)| {
                x >= left && x < left.saturating_add(w) && y >= 0 && y < h
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct UpDownState {
    pub buddy: u32,
    pub lower: i32,
    pub upper: i32,
    pub position: i32,
    pub base: u32,
    pub accelerators: Vec<(u32, u32)>,
}
impl Default for UpDownState {
    fn default() -> Self {
        Self {
            buddy: 0,
            lower: 0,
            upper: 100,
            position: 0,
            base: 10,
            accelerators: vec![(0, 1)],
        }
    }
}
impl UpDownState {
    pub fn clamp(&self, value: i32) -> i32 {
        value.clamp(self.lower.min(self.upper), self.lower.max(self.upper))
    }
    pub fn step(&self, delta: i32, wrap: bool) -> i32 {
        let value = i64::from(self.position) + i64::from(delta);
        let min = i64::from(self.lower.min(self.upper));
        let max = i64::from(self.lower.max(self.upper));
        if wrap {
            (min + (value - min).rem_euclid(max - min + 1)) as i32
        } else {
            value.clamp(min, max) as i32
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum CommonControlChange {
    TreeSelect { item: u32, reason: u32 },
    TreeExpand { item: u32, action: u32 },
    UpDown { delta: i32 },
}
#[derive(Debug, Clone)]
pub struct CommonControlFrame {
    pub thread: usize,
    pub thunk: u32,
    pub frame: crate::GuestCallFrame,
    pub hwnd: u32,
    pub change: CommonControlChange,
    pub scratch: u32,
    pub old_item: Option<TreeItem>,
    pub stage: u32,
    pub modal: bool,
    pub result: u32,
}

impl ChildWindow {
    pub fn tree_hit(&self, x: i32, y: i32) -> Option<(u32, bool)> {
        if x < 0 || y < 0 || x >= self.w || y >= self.h {
            return None;
        }
        let row = self.tree.first_visible + (y / self.tree.row_height.max(1)) as usize;
        let (item, depth) = *self.tree.visible().get(row)?;
        let button_x = 4 + depth * self.tree.indent;
        let node = &self.tree.items[&item];
        Some((
            item,
            (button_x..button_x + 11).contains(&x)
                && (!node.children.is_empty() || node.children_hint > 0)
                && self.style & 1 != 0,
        ))
    }
    pub fn tree_rect(&self, handle: u32, text_only: bool) -> Option<[i32; 4]> {
        let rows = self.tree.visible();
        let (row, &(_, depth)) = rows.iter().enumerate().find(|(_, (h, _))| *h == handle)?;
        let y = (row as i32 - self.tree.first_visible as i32) * self.tree.row_height;
        let x = if text_only {
            18 + depth * self.tree.indent
        } else {
            0
        };
        Some([x, y, self.w, y + self.tree.row_height])
    }
    pub(super) fn common_pointer(&mut self, x: i32, y: i32) -> Option<ControlAction> {
        let (code, index) = match self.class {
            ControlClass::TreeView => {
                let (item, expand) = self.tree_hit(x, y)?;
                (if expand { -454i32 } else { -450i32 }, item as i32)
            }
            ControlClass::Header => (-322, self.header.hit(x, y, self.h)? as i32),
            ControlClass::UpDown => {
                let forward = if self.style & 0x40 != 0 {
                    x >= self.w / 2
                } else {
                    y < self.h / 2
                };
                let delta = if forward { 1 } else { -1 }
                    * if self.updown.upper >= self.updown.lower {
                        1
                    } else {
                        -1
                    };
                (-722, delta)
            }
            _ => return None,
        };
        Some(ControlAction::Notify {
            parent: self.parent,
            id: self.id,
            hwnd: self.hwnd,
            code: code as u32,
            index,
        })
    }
    pub(super) fn common_key(&mut self, vk: u16) -> Option<ControlAction> {
        let (code, index) = match self.class {
            ControlClass::UpDown if matches!(vk, 0x25..=0x28) && self.style & 0x20 != 0 => {
                let delta = if vk == 0x26 || vk == 0x27 { 1 } else { -1 }
                    * if self.updown.upper >= self.updown.lower {
                        1
                    } else {
                        -1
                    };
                (-722, delta)
            }
            ControlClass::TreeView if matches!(vk, 0x23..=0x28) => {
                let rows = self.tree.visible();
                let selected = self.tree.selected;
                let index = rows.iter().position(|(h, _)| *h == selected).unwrap_or(0);
                let item = self.tree.items.get(&selected);
                if vk == 0x25 && item.is_some_and(|i| i.state & 0x20 != 0)
                    || vk == 0x27
                        && item.is_some_and(|i| {
                            i.state & 0x20 == 0 && (!i.children.is_empty() || i.children_hint > 0)
                        })
                {
                    (-454, selected as i32)
                } else {
                    let handle = match vk {
                        0x23 => rows.last().map_or(0, |(h, _)| *h),
                        0x24 => rows.first().map_or(0, |(h, _)| *h),
                        0x25 => {
                            item.map_or(
                                selected,
                                |i| if i.parent == 0 { selected } else { i.parent },
                            )
                        }
                        0x27 => item
                            .and_then(|i| i.children.first())
                            .copied()
                            .unwrap_or(selected),
                        0x26 => rows.get(index.saturating_sub(1)).map_or(0, |(h, _)| *h),
                        _ => rows
                            .get((index + 1).min(rows.len().saturating_sub(1)))
                            .map_or(0, |(h, _)| *h),
                    };
                    (-450, handle as i32)
                }
            }
            _ => return None,
        };
        Some(ControlAction::Notify {
            parent: self.parent,
            id: self.id,
            hwnd: self.hwnd,
            code: code as u32,
            index,
        })
    }
    pub(super) fn render_tree(&self, surf: &mut Surface<'_>, gdi: Option<&crate::gdi::GdiState>) {
        surf.fill_rect(
            self.x,
            self.y,
            self.w,
            self.h,
            crate::framebuffer::colorref_to_rgb565(self.tree.background),
        );
        let face = gdi.and_then(|g| crate::text::selected(g, self.font));
        let clip = [
            self.x,
            self.y,
            self.x.saturating_add(self.w),
            self.y.saturating_add(self.h),
        ];
        let list = gdi.and_then(|g| g.image_lists.get(&self.tree.image_lists[0]));
        for (row, (handle, depth)) in self
            .tree
            .visible()
            .into_iter()
            .skip(self.tree.first_visible)
            .enumerate()
        {
            let y = self.y + row as i32 * self.tree.row_height;
            if y >= clip[3] {
                break;
            }
            let item = &self.tree.items[&handle];
            let x = self.x + 4 + depth * self.tree.indent;
            let selected = self.tree.selected == handle;
            if selected {
                surf.fill_rect(
                    self.x,
                    y,
                    self.w,
                    self.tree.row_height.min(clip[3] - y),
                    0x0010,
                );
            }
            let cy = y + self.tree.row_height / 2;
            if self.style & 1 != 0
                && (!item.children.is_empty() || item.children_hint > 0)
                && x + 9 < clip[2]
                && cy + 4 < clip[3]
            {
                stroke(surf, x, cy - 4, 9, 9, Self::TEXT);
                surf.fill_rect(x + 2, cy, 5, 1, Self::TEXT);
                if item.state & 0x20 == 0 {
                    surf.fill_rect(x + 4, cy - 2, 1, 5, Self::TEXT);
                }
            }
            let mut text_x = x + 14;
            let index = if selected {
                item.selected_image
            } else {
                item.image
            };
            if let Some(image) = list.and_then(|l| {
                usize::try_from(index)
                    .ok()
                    .and_then(|i| l.image(i, self.tree.item_state(item)))
            }) {
                let w = image.width;
                let h = image.height;
                image.draw(
                    surf,
                    text_x,
                    y + (self.tree.row_height - h as i32) / 2,
                    w,
                    h,
                    clip,
                );
                text_x += w as i32 + 3;
            }
            let height = crate::text::layout(face.as_ref(), &[]).height;
            let line =
                crate::text::layout(face.as_ref(), &item.text.encode_utf16().collect::<Vec<_>>());
            line.draw(
                surf,
                text_x,
                y + (self.tree.row_height - height) / 2,
                if selected {
                    Self::WINDOW
                } else {
                    crate::framebuffer::colorref_to_rgb565(self.tree.text_color)
                },
                clip,
            );
        }
    }
    pub(super) fn render_header(&self, surf: &mut Surface<'_>, face: Option<&crate::gdi::Font>) {
        if self.style & 8 != 0 {
            return;
        }
        surf.fill_rect(self.x, self.y, self.w, self.h, Self::FACE);
        for &index in &self.header.order {
            let Some((left, _, width, _)) = self.header.rect(index, self.h) else {
                continue;
            };
            if left >= self.w {
                break;
            }
            let width = width.min(self.w - left);
            let x = self.x + left;
            draw_edge(surf, x, self.y, width, self.h, Self::LIGHT, Self::SHADOW);
            let line = crate::text::layout(
                face,
                &self.header.items[index]
                    .text
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            );
            let tx = match self.header.items[index].format & 3 {
                1 => x + width - line.width - 3,
                2 => x + (width - line.width) / 2,
                _ => x + 3,
            };
            line.draw(
                surf,
                tx,
                self.y + (self.h - line.height) / 2,
                Self::TEXT,
                [x + 1, self.y + 1, x + width - 1, self.y + self.h - 1],
            );
        }
    }
    pub(super) fn render_updown(&self, surf: &mut Surface<'_>) {
        let horizontal = self.style & 0x40 != 0;
        for n in 0..2 {
            let (x, y, w, h) = if horizontal {
                (self.x + n * self.w / 2, self.y, self.w / 2, self.h)
            } else {
                (self.x, self.y + n * self.h / 2, self.w, self.h / 2)
            };
            surf.fill_rect(x, y, w, h, Self::FACE);
            draw_edge(surf, x, y, w, h, Self::LIGHT, Self::SHADOW);
            let color = if self.enabled {
                Self::TEXT
            } else {
                Self::SHADOW
            };
            for i in 0..3 {
                let direction = if n == 0 { -1 } else { 1 };
                if horizontal {
                    surf.fill_rect(
                        x + w / 2 + direction * (2 - i),
                        y + h / 2 - i,
                        1,
                        2 * i + 1,
                        color,
                    );
                } else {
                    surf.fill_rect(
                        x + w / 2 - i,
                        y + h / 2 + direction * (2 - i),
                        2 * i + 1,
                        1,
                        color,
                    );
                }
            }
        }
    }
}
