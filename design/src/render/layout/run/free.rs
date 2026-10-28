//! Free runs: frames with no layout, whose children each sit at their own
//! `x` and `y`. Each child is laid out alone against the frame's box, and
//! the frame hugs the furthest any of them reaches — counted, so a child's
//! change costs that child.

use super::*;

impl Renderer {
    /// A free run's size under what its parent fixes of it: its own box
    /// where fixed, else the furthest its children reach — at least its
    /// padding, as taffy floors a box — found as taffy's settle finds it:
    /// the children laid out against the box from its padding up, again
    /// while it grows. Only the children whose size can depend on the box
    /// are laid out again for each box; the rest once.
    pub(super) fn measure_free(
        &mut self,
        runs: &mut Runs,
        id: u32,
        known: Size<Option<f32>>,
    ) -> Size<f32> {
        let mut run = runs.take(id);
        let [t, r, b, l] = run.padding;
        let (pw, ph) = (l + r, t + b);
        // As taffy sizes a frame whose children are all placed — a flex row
        // with no items: along, what it is given as it is; across, never
        // less than its padding.
        let known = Size {
            width: known.width.map(|w| w.max(pw)),
            height: known.height.map(|h| h.max(ph)),
        };
        let mut cb = (known.width.unwrap_or(pw), known.height.unwrap_or(ph));
        let stale: Vec<OrderKey> = std::mem::take(&mut run.stale)
            .into_iter()
            .filter(|k| run.children.contains_key(k))
            .collect();
        run.gone = false;
        for k in &stale {
            run.relaid.insert(k.clone());
            self.free_child(runs, &mut run, k, cb);
        }
        let (hw, hh) = run.hug;
        for _ in 0..64 {
            let new = (
                known.width.unwrap_or(if hw {
                    greatest(&run.rights).max(pw)
                } else {
                    pw
                }),
                known.height.unwrap_or(if hh {
                    greatest(&run.bottoms).max(ph)
                } else {
                    ph
                }),
            );
            if new == cb {
                break;
            }
            cb = new;
            let sensitive: Vec<OrderKey> = run.overs.iter().cloned().collect();
            for k in &sensitive {
                self.free_child(runs, &mut run, k, cb);
            }
        }
        // What a child whose size can depend on the box last laid out
        // against is what the box now is.
        let sensitive: Vec<OrderKey> = run.overs.iter().cloned().collect();
        for k in &sensitive {
            self.free_child(runs, &mut run, k, cb);
        }
        run.cb = cb;
        runs.put(id, run);
        Size {
            width: cb.0,
            height: cb.1,
        }
    }

    /// A free run's child laid out against the box `cb`, its box counted:
    /// once for a child whose size can't depend on it, else for each box.
    fn free_child(&mut self, runs: &mut Runs, run: &mut RunLayout, key: &OrderKey, cb: (f32, f32)) {
        let sens = run.children[key].sens;
        if sens {
            run.overs.insert(key.clone());
        } else {
            run.overs.remove(key);
        }
        let shape = Shape {
            room: AvailableSpace::Definite(cb.0),
            least: false,
            line: Some(cb.1),
            basis: None,
            main: None,
        };
        let (wrap, node, last) = {
            let c = &run.children[key];
            (c.wrap, c.node, c.last)
        };
        if last.is_some() && (!sens || last == Some(shape)) {
            return;
        }
        let mut style = run.tree.style(wrap).expect("style").clone();
        style.size = Size {
            width: Dimension::length(cb.0),
            height: Dimension::length(cb.1),
        };
        run.tree.set_style(wrap, style).expect("style");
        let available = Size {
            width: AvailableSpace::Definite(cb.0),
            height: AvailableSpace::Definite(cb.1),
        };
        self.settle_in(runs, &mut run.tree, wrap, available, &[]);
        run.children.get_mut(key).expect("a child").last = Some(shape);
        run.relaid.insert(key.clone());
        let l = run.tree.unrounded_layout(node);
        let b = [l.location.x, l.location.y, l.size.width, l.size.height];
        if let Some(old) = run.boxes.insert(key.clone(), b) {
            uncount(&mut run.rights, old[0] + old[2]);
            uncount(&mut run.bottoms, old[1] + old[3]);
        }
        count(&mut run.rights, b[0] + b[2]);
        count(&mut run.bottoms, b[1] + b[3]);
    }

    /// A free run's child laid out against the run's box as it now is, so
    /// its layout nodes hold the box the drawing reads.
    pub(in crate::render) fn relay_free(&mut self, runs: &mut Runs, rid: u32, key: &OrderKey) {
        let mut run = runs.take(rid);
        let cb = run.cb;
        self.free_child(runs, &mut run, key, cb);
        runs.put(rid, run);
    }
}
