//! Prototyping: links from layers to artboards, hotspots, flow start points, and the player that
//! walks through a prototype in the preview window.
//!
//! Any layer can link to an artboard on the same page (or back to the previous
//! one). Clicking anywhere inside a linked layer, or inside a layer within a linked group, follows
//! the link; layers without links don't block clicks on links underneath them.

use kurbo::{Affine, BezPath, Point, Rect, Shape, Vec2};
use serde::{Deserialize, Serialize};

use crate::{Layer, LayerId, LayerKind, Page};

/// Where a link goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Destination {
    Artboard(LayerId),
    /// The artboard shown before the current one.
    Back,
}

/// How the destination comes in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Animation {
    None,
    #[default]
    SlideFromRight,
    SlideFromLeft,
    SlideFromBottom,
    SlideFromTop,
    /// Fades the destination in over the current artboard.
    Dissolve,
    /// Slides in from the right, pushing the current artboard out to the left.
    PushFromRight,
    /// Slides in from the left, pushing the current artboard out to the right.
    PushFromLeft,
}

impl Animation {
    pub const ALL: [Animation; 8] = [
        Animation::None,
        Animation::SlideFromRight,
        Animation::SlideFromLeft,
        Animation::SlideFromBottom,
        Animation::SlideFromTop,
        Animation::Dissolve,
        Animation::PushFromRight,
        Animation::PushFromLeft,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Animation::None => "No Animation",
            Animation::SlideFromRight => "Slide from Right",
            Animation::SlideFromLeft => "Slide from Left",
            Animation::SlideFromBottom => "Slide from Bottom",
            Animation::SlideFromTop => "Slide from Top",
            Animation::Dissolve => "Dissolve",
            Animation::PushFromRight => "Push from Right",
            Animation::PushFromLeft => "Push from Left",
        }
    }

    /// Whether the outgoing artboard moves too.
    fn pushes(self) -> bool {
        matches!(self, Animation::PushFromRight | Animation::PushFromLeft)
    }

    /// The direction the incoming artboard starts from, as a fraction of the viewport size.
    fn start_offset(self) -> Vec2 {
        match self {
            Animation::None | Animation::Dissolve => Vec2::ZERO,
            Animation::SlideFromRight | Animation::PushFromRight => Vec2::new(1.0, 0.0),
            Animation::SlideFromLeft | Animation::PushFromLeft => Vec2::new(-1.0, 0.0),
            Animation::SlideFromBottom => Vec2::new(0.0, 1.0),
            Animation::SlideFromTop => Vec2::new(0.0, -1.0),
        }
    }
}

/// A prototyping link on a layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowLink {
    pub destination: Destination,
    #[serde(default)]
    pub animation: Animation,
    /// Shows the destination on top of the current artboard, centered, instead of replacing it.
    /// Clicking outside it or following a Back link closes it.
    #[serde(default, skip_serializing_if = "crate::prototype::is_false")]
    pub overlay: bool,
}

fn is_false(b: &bool) -> bool {
    !b
}

impl FlowLink {
    pub fn to(artboard: LayerId) -> Self {
        Self { destination: Destination::Artboard(artboard), animation: Animation::default(), overlay: false }
    }

    pub fn back() -> Self {
        Self { destination: Destination::Back, animation: Animation::default(), overlay: false }
    }
}

/// The artboard at the root of the page that contains `id` (or is `id`).
pub fn artboard_of(page: &Page, id: LayerId) -> Option<LayerId> {
    let root = page.ancestors(id)?.first().copied().unwrap_or(id);
    page.find(root).filter(|l| l.is_artboard()).map(|l| l.id)
}

/// Visible, non-master artboards on the page, in layer-list order (top first).
pub fn artboards(page: &Page) -> Vec<&Layer> {
    page.layers.iter().rev().filter(|l| l.is_artboard() && l.master_of().is_none()).collect()
}

/// Artboards marked as flow start points.
pub fn flow_starts(page: &Page) -> Vec<LayerId> {
    artboards(page).into_iter().filter(|l| matches!(&l.kind, LayerKind::Artboard(a) if a.flow_home)).map(|l| l.id).collect()
}

/// Where previewing starts: the artboard holding the selection, else the first start point, else
/// the top-left artboard.
pub fn start_artboard(page: &Page, selection: &[LayerId]) -> Option<LayerId> {
    let is_target = |id: LayerId| page.find(id).is_some_and(|l| l.is_artboard() && l.master_of().is_none());
    selection
        .iter()
        .filter_map(|&id| artboard_of(page, id))
        .find(|&id| is_target(id))
        .or_else(|| flow_starts(page).first().copied())
        .or_else(|| {
            let mut boards = artboards(page);
            boards.sort_by(|a, b| (a.frame.y, a.frame.x).partial_cmp(&(b.frame.y, b.frame.x)).unwrap_or(std::cmp::Ordering::Equal));
            boards.first().map(|l| l.id)
        })
}

/// Every layer on the page with a link, including hotspots.
pub fn links(page: &Page) -> Vec<(LayerId, FlowLink)> {
    let mut out = vec![];
    page.walk(|l| {
        if let Some(link) = l.flow {
            out.push((l.id, link));
        }
    });
    out
}

/// The artboard `id` on any page of `doc`, with its page.
pub fn find_artboard(doc: &crate::Document, id: LayerId) -> Option<(crate::PageId, &Layer)> {
    doc.pages.iter().find_map(|p| p.layers.iter().find(|l| l.id == id && l.is_artboard()).map(|l| (p.id, l)))
}

/// Whether `link` leads somewhere: back, or to an artboard that still exists on any page.
pub fn resolves(doc: &crate::Document, link: &FlowLink) -> bool {
    match link.destination {
        Destination::Back => true,
        Destination::Artboard(id) => find_artboard(doc, id).is_some(),
    }
}

/// A clickable area of an artboard in the preview, in viewport coordinates (the artboard's
/// top-left at the origin, before scaling).
#[derive(Clone, Debug)]
pub struct LinkArea {
    pub layer: LayerId,
    pub link: FlowLink,
    /// The layer's frame outline.
    pub outline: BezPath,
}

/// The link areas of `artboard`, topmost first, with the artboard scrolled down by `scroll`.
/// Layers fixed to the viewport don't scroll, and sit above the rest.
pub fn link_areas(artboard: &Layer, scroll: f64) -> Vec<LinkArea> {
    let Some(children) = artboard.children() else { return vec![] };
    let mut out = vec![];
    // Fixed layers (at any depth) sit above the rest and don't scroll.
    for fixed_pass in [true, false] {
        for l in children.iter().rev() {
            collect_areas(l, Affine::IDENTITY, None, scroll, fixed_pass, false, &mut out);
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn collect_areas(layer: &Layer, parent: Affine, inherited: Option<FlowLink>, scroll: f64, fixed_pass: bool, in_fixed: bool, out: &mut Vec<LinkArea>) {
    if !layer.visible {
        return;
    }
    let in_fixed = in_fixed || layer.fixed_to_viewport;
    let t = parent * layer.local_transform();
    let link = layer.flow.or(inherited);
    let shift = Affine::translate((0.0, if in_fixed { 0.0 } else { -scroll }));
    let push = |link: FlowLink, out: &mut Vec<LinkArea>| {
        if in_fixed == fixed_pass {
            out.push(LinkArea { layer: layer.id, link, outline: shift * t * layer.frame.local_rect().to_path(0.1) });
        }
    };
    let children = match &layer.kind {
        LayerKind::Symbol(s) => Some(&s.expanded),
        _ => layer.children(),
    };
    match children {
        // A linked group's area is its children's, so its link wins over theirs only where
        // they have none.
        Some(children) => {
            for c in children.iter().rev() {
                collect_areas(c, t, link, scroll, fixed_pass, in_fixed, out);
            }
            if let (Some(link), LayerKind::Symbol(_)) = (layer.flow, &layer.kind) {
                push(link, out);
            }
        }
        None => {
            if let Some(link) = link {
                push(link, out);
            }
        }
    }
}

/// An artboard split for scrolling: itself without its fixed layers, and those layers (from any
/// depth) moved into the artboard's own space, bottom to top.
pub fn split_fixed(artboard: &Layer) -> (Layer, Vec<Layer>) {
    fn lift(layers: &mut Vec<Layer>, parent: Affine, out: &mut Vec<Layer>) {
        let mut i = 0;
        while i < layers.len() {
            if layers[i].fixed_to_viewport {
                let mut l = layers.remove(i);
                place_in(&mut l, parent);
                out.push(l);
                continue;
            }
            let t = parent * layers[i].local_transform();
            if let Some(children) = layers[i].children_mut() {
                lift(children, t, out);
            }
            i += 1;
        }
    }
    let mut rest = artboard.clone();
    let mut fixed = vec![];
    if let Some(children) = rest.children_mut() {
        lift(children, Affine::IDENTITY, &mut fixed);
    }
    (rest, fixed)
}

/// Re-expresses `layer`, whose parent maps to artboard space by `parent` (rotation, flips and
/// translation), directly in artboard space.
fn place_in(layer: &mut Layer, parent: Affine) {
    let c = layer.frame.local_rect().center();
    let center = parent * layer.local_transform() * c;
    let [a, b, cc, d, _, _] = parent.as_coeffs();
    if a * d - b * cc < 0.0 {
        layer.flip_horizontal = !layer.flip_horizontal;
    }
    layer.rotation = (layer.rotation + b.atan2(a).to_degrees()).rem_euclid(360.0);
    layer.frame.x = center.x - layer.frame.width / 2.0;
    layer.frame.y = center.y - layer.frame.height / 2.0;
}

/// The link under `point` (viewport coordinates) on `artboard` scrolled by `scroll`.
pub fn link_at(artboard: &Layer, point: Point, scroll: f64) -> Option<FlowLink> {
    link_areas(artboard, scroll).into_iter().find(|a| a.outline.winding(point) != 0).map(|a| a.link)
}

/// How long transitions take, in seconds.
pub const TRANSITION_SECONDS: f64 = 0.3;

/// An in-progress change of artboard.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transition {
    pub from: LayerId,
    pub animation: Animation,
    /// Going back: the current artboard slides away to reveal the previous one.
    pub reverse: bool,
    pub started: f64,
}

/// Where the two artboards of a transition are drawn, as offsets in viewport units, and which is
/// on top.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frames {
    pub from: Vec2,
    pub to: Vec2,
    /// Draw `from` above `to` (when going back, the old artboard slides off the top).
    pub from_on_top: bool,
    /// Opacity of whichever artboard is on top (dissolves fade it).
    pub top_alpha: f64,
}

impl Transition {
    /// Eased progress, 0..1.
    pub fn progress(&self, now: f64) -> f64 {
        if self.animation == Animation::None {
            return 1.0;
        }
        let t = ((now - self.started) / TRANSITION_SECONDS).clamp(0.0, 1.0);
        1.0 - (1.0 - t).powi(3)
    }

    pub fn done(&self, now: f64) -> bool {
        self.progress(now) >= 1.0
    }

    /// Offsets of the outgoing and incoming artboards for a viewport of `size`.
    pub fn frames(&self, now: f64, size: Vec2) -> Frames {
        let p = self.progress(now);
        let d = self.animation.start_offset();
        let d = Vec2::new(d.x * size.x, d.y * size.y);
        let push = self.animation.pushes();
        match (self.animation, self.reverse) {
            // A dissolve fades the top artboard: in going forward, out going back.
            (Animation::Dissolve, false) => Frames { from: Vec2::ZERO, to: Vec2::ZERO, from_on_top: false, top_alpha: p },
            (Animation::Dissolve, true) => Frames { from: Vec2::ZERO, to: Vec2::ZERO, from_on_top: true, top_alpha: 1.0 - p },
            // The artboard we're leaving slides back out the way it came (pushing the other back).
            (_, true) => Frames { from: d * p, to: if push { -d * (1.0 - p) } else { Vec2::ZERO }, from_on_top: true, top_alpha: 1.0 },
            (_, false) => Frames { from: if push { -d * p } else { Vec2::ZERO }, to: d * (1.0 - p), from_on_top: false, top_alpha: 1.0 },
        }
    }
}

/// Plays a prototype: the artboard on show, how we got there, and any transition under way.
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub current: LayerId,
    pub start: LayerId,
    /// Artboards visited before the current one, with the animation that left each.
    history: Vec<(LayerId, Animation)>,
    pub transition: Option<Transition>,
    /// Vertical scroll of the current artboard, in artboard units.
    pub scroll: f64,
    /// An artboard shown over the current one, and when it appeared (for its fade-in).
    pub overlay: Option<(LayerId, f64)>,
}

impl Player {
    pub fn new(start: LayerId) -> Self {
        Self { current: start, start, history: vec![], transition: None, scroll: 0.0, overlay: None }
    }

    /// Closes the overlay. Returns whether there was one.
    pub fn close_overlay(&mut self) -> bool {
        self.overlay.take().is_some()
    }

    pub fn can_go_back(&self) -> bool {
        !self.history.is_empty()
    }

    /// Follows `link` from the current artboard at time `now`. Returns whether the artboard
    /// changed.
    pub fn follow(&mut self, doc: &crate::Document, link: FlowLink, now: f64) -> bool {
        match link.destination {
            // Back from an overlay closes it.
            Destination::Back if self.overlay.is_some() => self.close_overlay(),
            Destination::Back => self.back(now),
            Destination::Artboard(to) if link.overlay => {
                if find_artboard(doc, to).is_none() || to == self.current {
                    return false;
                }
                self.overlay = Some((to, now));
                true
            }
            Destination::Artboard(to) => {
                if to == self.current || find_artboard(doc, to).is_none() {
                    return false;
                }
                // Leaving for another artboard closes any overlay.
                self.overlay = None;
                self.history.push((self.current, link.animation));
                self.transition = Some(Transition { from: self.current, animation: link.animation, reverse: false, started: now });
                self.current = to;
                self.scroll = 0.0;
                true
            }
        }
    }

    /// Returns to the previous artboard, reversing the animation that left it.
    pub fn back(&mut self, now: f64) -> bool {
        if self.close_overlay() {
            return true;
        }
        let Some((to, animation)) = self.history.pop() else { return false };
        self.transition = Some(Transition { from: self.current, animation, reverse: true, started: now });
        self.current = to;
        self.scroll = 0.0;
        true
    }

    /// Jumps straight to `artboard`, forgetting the history.
    pub fn jump(&mut self, artboard: LayerId) {
        self.current = artboard;
        self.history.clear();
        self.transition = None;
        self.scroll = 0.0;
        self.overlay = None;
    }

    pub fn restart(&mut self) {
        let start = self.start;
        self.jump(start);
    }

    /// Ends the transition once it has played. Returns whether one is still running.
    pub fn tick(&mut self, now: f64) -> bool {
        if self.transition.is_some_and(|t| t.done(now)) {
            self.transition = None;
        }
        self.transition.is_some()
    }

    /// Forgets artboards that no longer exist, e.g. after an edit in the main window. Returns
    /// false if the current one is gone too.
    pub fn sanitize(&mut self, doc: &crate::Document) -> bool {
        let exists = |id: LayerId| find_artboard(doc, id).is_some();
        if self.overlay.is_some_and(|(o, _)| !exists(o)) {
            self.overlay = None;
        }
        self.history.retain(|&(id, _)| exists(id));
        if self.transition.is_some_and(|t| !exists(t.from)) {
            self.transition = None;
        }
        exists(self.current)
    }
}

/// The largest scroll for `artboard` shown in a viewport `height` units tall.
pub fn max_scroll(artboard: &Layer, height: f64) -> f64 {
    (artboard.frame.height - height).max(0.0)
}

/// The bounds of a link's source layer and its destination artboard on the page, for drawing
/// the connection on the canvas.
/// The bounds of a link's source layer and its destination artboard, if that's on the same page.
pub fn connection(page: &Page, source: LayerId, link: &FlowLink) -> Option<(Rect, Option<Rect>)> {
    let from = page.world_bounds(source)?;
    let to = match link.destination {
        Destination::Artboard(id) => page.world_bounds(id),
        Destination::Back => None,
    };
    Some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Frame, ops};

    /// Two artboards: "Home" with a linked button and a hotspot, and "Detail".
    fn sample() -> (Page, LayerId, LayerId, LayerId) {
        let mut page = Page::new("P");
        let home = ops::insert_drawn(&mut page, Layer::artboard("Home", Frame::new(0.0, 0.0, 300.0, 600.0)));
        let detail = ops::insert_drawn(&mut page, Layer::artboard("Detail", Frame::new(400.0, 0.0, 300.0, 600.0)));
        let mut button = Layer::rectangle(Frame::new(20.0, 20.0, 100.0, 40.0));
        button.flow = Some(FlowLink::to(detail));
        let button = ops::insert_drawn(&mut page, button);
        let mut back = Layer::hotspot(Frame::new(420.0, 20.0, 40.0, 40.0));
        back.flow = Some(FlowLink::back());
        ops::insert_drawn(&mut page, back);
        (page, home, detail, button)
    }

    #[test]
    fn links_are_found_inside_their_artboard() {
        let (page, home, detail, button) = sample();
        assert_eq!(page.parent(button), Some(Some(home)));
        let ab = page.find(home).unwrap();
        assert_eq!(link_at(ab, Point::new(30.0, 30.0), 0.0), Some(FlowLink::to(detail)));
        assert_eq!(link_at(ab, Point::new(200.0, 300.0), 0.0), None);
        // Scrolled down by 30, the button sits higher.
        assert_eq!(link_at(ab, Point::new(30.0, 30.0), 30.0), None);
        assert_eq!(link_at(ab, Point::new(30.0, 0.0), 30.0), Some(FlowLink::to(detail)));
        let detail_ab = page.find(detail).unwrap();
        assert_eq!(link_at(detail_ab, Point::new(30.0, 30.0), 0.0), Some(FlowLink::back()));
        assert_eq!(links(&page).len(), 2);
    }

    #[test]
    fn group_links_cover_their_children_and_plain_layers_dont_block() {
        let (mut page, home, detail, button) = sample();
        // An unlinked rectangle over the button.
        let cover = ops::insert_drawn(&mut page, Layer::rectangle(Frame::new(0.0, 0.0, 300.0, 100.0)));
        assert_eq!(page.parent(cover), Some(Some(home)));
        let ab = page.find(home).unwrap();
        assert_eq!(link_at(ab, Point::new(30.0, 30.0), 0.0), Some(FlowLink::to(detail)));
        // Group the button with a label; link the group back instead.
        let label = ops::insert_drawn(&mut page, Layer::rectangle(Frame::new(20.0, 200.0, 50.0, 20.0)));
        let group = ops::group(&mut page, &[button, label]).unwrap();
        page.find_mut(group).unwrap().flow = Some(FlowLink::back());
        let ab = page.find(home).unwrap();
        assert_eq!(link_at(ab, Point::new(30.0, 205.0), 0.0), Some(FlowLink::back()));
        // The button's own link still wins over the group's.
        assert_eq!(link_at(ab, Point::new(30.0, 30.0), 0.0), Some(FlowLink::to(detail)));
        // Between the children isn't inside either.
        assert_eq!(link_at(ab, Point::new(30.0, 120.0), 0.0), None);
    }

    #[test]
    fn fixed_layers_ignore_scroll_and_sit_on_top() {
        let (mut page, home, detail, _) = sample();
        let mut bar = Layer::hotspot(Frame::new(0.0, 0.0, 300.0, 50.0));
        bar.fixed_to_viewport = true;
        bar.flow = Some(FlowLink::back());
        ops::insert_drawn(&mut page, bar);
        // Move the fixed bar below the button in z-order: it still wins.
        let ab = page.find_mut(home).unwrap();
        let children = ab.children_mut().unwrap();
        let bar = children.pop().unwrap();
        children.insert(0, bar);
        let ab = page.find(home).unwrap();
        assert_eq!(link_at(ab, Point::new(30.0, 30.0), 0.0), Some(FlowLink::back()));
        // Scrolled, the button passes under the bar while the bar stays put.
        assert_eq!(link_at(ab, Point::new(30.0, 10.0), 500.0), Some(FlowLink::back()));
        assert_eq!(link_at(ab, Point::new(30.0, 55.0), 0.0), Some(FlowLink::to(detail)));
        assert_eq!(link_at(ab, Point::new(30.0, 25.0), 0.0), Some(FlowLink::back()));
    }

    #[test]
    fn fixed_layers_inside_groups() {
        let (mut page, home, detail, _) = sample();
        // A fixed tab bar nested in a group near the bottom.
        let mut tab = Layer::hotspot(Frame::new(0.0, 550.0, 300.0, 50.0));
        tab.flow = Some(FlowLink::to(detail));
        tab.fixed_to_viewport = true;
        let tab_id = ops::insert_drawn(&mut page, tab);
        let other = ops::insert_drawn(&mut page, Layer::rectangle(Frame::new(0.0, 500.0, 10.0, 10.0)));
        ops::group(&mut page, &[tab_id, other]).unwrap();
        let ab = page.find(home).unwrap();
        // Scrolled by 200, the bar stays at the bottom of the viewport.
        assert_eq!(link_at(ab, Point::new(100.0, 575.0), 200.0), Some(FlowLink::to(detail)));
        let (rest, fixed) = split_fixed(ab);
        assert_eq!(fixed.len(), 1);
        assert!((fixed[0].frame.y - 550.0).abs() < 1e-9 && (fixed[0].frame.x).abs() < 1e-9, "{:?}", fixed[0].frame);
        let mut ids = vec![];
        rest.walk(&mut |l| ids.push(l.id));
        assert!(!ids.contains(&tab_id));
    }

    #[test]
    fn player_follows_links_and_goes_back() {
        let (page, home, detail, _) = sample();
        let doc = crate::Document { pages: vec![page], ..Default::default() };
        let mut p = Player::new(home);
        assert!(!p.can_go_back());
        assert!(p.follow(&doc, FlowLink::to(detail), 0.0));
        assert_eq!(p.current, detail);
        let t = p.transition.unwrap();
        assert_eq!((t.from, t.reverse), (home, false));
        // Halfway the incoming artboard is on its way in from the right.
        let f = t.frames(0.1, Vec2::new(300.0, 600.0));
        assert!(f.to.x > 0.0 && f.to.x < 300.0 && f.from == Vec2::ZERO);
        assert!(p.tick(0.1));
        assert!(!p.tick(1.0));
        assert!(p.follow(&doc, FlowLink::back(), 2.0));
        assert_eq!(p.current, home);
        assert!(p.transition.unwrap().reverse);
        assert!(!p.back(3.0));
        // Links to the current artboard or to nothing do nothing.
        assert!(!p.follow(&doc, FlowLink::to(home), 4.0));
        assert!(!p.follow(&doc, FlowLink::to(LayerId::new()), 4.0));
    }

    #[test]
    fn overlays_and_links_across_pages() {
        let (page, home, detail, _) = sample();
        let mut other = Page::new("Other");
        let settings = ops::insert_drawn(&mut other, Layer::artboard("Settings", Frame::new(0.0, 0.0, 200.0, 200.0)));
        let doc = crate::Document { pages: vec![page, other], ..Default::default() };
        let mut p = Player::new(home);
        // An overlay sits over the current artboard without leaving it.
        assert!(p.follow(&doc, FlowLink { overlay: true, ..FlowLink::to(detail) }, 0.0));
        assert_eq!((p.current, p.overlay.map(|o| o.0)), (home, Some(detail)));
        assert!(!p.can_go_back());
        // Back closes it.
        assert!(p.follow(&doc, FlowLink::back(), 1.0));
        assert!(p.overlay.is_none() && p.current == home);
        // A link to another page's artboard works, and resolves.
        let link = FlowLink::to(settings);
        assert!(resolves(&doc, &link));
        assert!(p.follow(&doc, link, 2.0));
        assert_eq!(find_artboard(&doc, p.current).map(|(pg, _)| pg), Some(doc.pages[1].id));
        assert!(p.sanitize(&doc));
    }

    #[test]
    fn dissolves_fade_and_pushes_move_both() {
        let size = Vec2::new(100.0, 100.0);
        let t = |animation, reverse| Transition { from: LayerId::new(), animation, reverse, started: 0.0 };
        let f = t(Animation::Dissolve, false).frames(0.1, size);
        assert!(f.top_alpha > 0.0 && f.top_alpha < 1.0 && f.to == Vec2::ZERO && !f.from_on_top);
        let f = t(Animation::PushFromRight, false).frames(0.1, size);
        assert!(f.from.x < 0.0 && f.to.x > 0.0, "the old one leaves left as the new one comes in: {f:?}");
        assert!((f.to.x - f.from.x - 100.0).abs() < 1e-9, "edge to edge");
        let f = t(Animation::SlideFromRight, false).frames(0.1, size);
        assert_eq!(f.from, Vec2::ZERO, "slides leave the old one in place");
    }

    #[test]
    fn start_prefers_selection_then_flow_home() {
        let (mut page, home, detail, button) = sample();
        assert_eq!(start_artboard(&page, &[]), Some(home));
        if let LayerKind::Artboard(a) = &mut page.find_mut(detail).unwrap().kind {
            a.flow_home = true;
        }
        assert_eq!(flow_starts(&page), vec![detail]);
        assert_eq!(start_artboard(&page, &[]), Some(detail));
        assert_eq!(start_artboard(&page, &[button]), Some(home));
    }

    #[test]
    fn links_survive_the_file_format() {
        let (page, ..) = sample();
        let json = serde_json::to_string(&page).unwrap();
        let back: Page = serde_json::from_str(&json).unwrap();
        assert_eq!(back, page);
        assert!(json.contains("\"destination\":{\"artboard\""));
    }
}
