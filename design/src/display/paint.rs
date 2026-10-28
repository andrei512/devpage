//! The one paint entry point: an area of the list at a scale into a
//! [`Target`] — the port both backends adapt — opening and closing exactly
//! the layers each command's property state needs.

use kurbo::{Affine, BezPath, Rect as KRect};

use crate::render::Draw;

use super::*;

/// What a backend does with the commands [`paint`] hands it, each already
/// under the transform from its coordinates to the target's pixels. A
/// backend translates each into its library's call and decides nothing.
pub trait Target {
    fn draw(&mut self, draw: &Draw, t: Affine);
    fn push_clip(&mut self, path: &BezPath, t: Affine);
    /// A layer with `blend`, bounded by `bounds` under `t` — or, when
    /// `None`, by `area`, the whole of what is painted, in the target's
    /// pixels.
    fn push_layer(&mut self, blend: Blend, bounds: Option<&BezPath>, t: Affine, area: KRect);
    fn pop(&mut self);
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub(super) enum Layer {
    Clip(Id),
    Effect(Id),
}

/// The engine's one paint entry point: `area` of the list — in the design's
/// coordinates — at `scale` device pixels per design pixel into `target`,
/// whose top-left is the area's. The commands that touch it, in paint order,
/// each under exactly the layers its property state needs.
pub fn paint(list: &List, area: KRect, scale: f64, target: &mut impl Target) {
    let view = Affine::scale(scale) * Affine::translate((-area.x0, -area.y0));
    let pixels = KRect::new(0.0, 0.0, area.width() * scale, area.height() * scale);
    let mut open: Vec<Layer> = Vec::new();
    let place = |transform: Id| view * Affine::translate(list.world(transform));
    for (origin, _, command) in list.query(area, 1.0 / scale) {
        let want = layers(list, command.clip, command.effect);
        let keep = open.iter().zip(&want).take_while(|(a, b)| a == b).count();
        while open.len() > keep {
            open.pop();
            target.pop();
        }
        for layer in &want[keep..] {
            match *layer {
                Layer::Clip(id) => {
                    let c = list.trees.clips.get(id);
                    target.push_clip(&c.path, place(c.transform));
                }
                Layer::Effect(id) => {
                    let e = list.trees.effects.get(id);
                    target.push_layer(e.blend, e.bounds.as_ref(), place(e.transform), pixels);
                }
            }
            open.push(*layer);
        }
        target.draw(&command.draw, view * Affine::translate(origin));
    }
    for _ in open {
        target.pop();
    }
}

/// The layers a command under `clip` and `effect` is drawn in, outermost
/// first: each effect from the root down, inside its own clip, then the
/// command's clips inside the innermost effect.
pub(super) fn layers(list: &List, clip: Id, effect: Option<Id>) -> Vec<Layer> {
    let mut effects = Vec::new();
    let mut at = effect;
    while let Some(e) = at {
        effects.push(e);
        at = list.trees.effects.get(e).parent;
    }
    effects.reverse();
    let mut out = Vec::new();
    let mut inside: Option<Id> = None;
    for e in effects {
        let to = list.trees.effects.get(e).clip;
        clips(list, inside, to, &mut out);
        out.push(Layer::Effect(e));
        inside = Some(to);
    }
    clips(list, inside, clip, &mut out);
    out
}

/// The clips from just below `from` down to `to`, outermost first.
pub(super) fn clips(list: &List, from: Option<Id>, to: Id, out: &mut Vec<Layer>) {
    let mut chain = Vec::new();
    let mut at = Some(to);
    while let Some(c) = at {
        if Some(c) == from {
            break;
        }
        chain.push(Layer::Clip(c));
        at = list.trees.clips.get(c).parent;
    }
    out.extend(chain.into_iter().rev());
}
