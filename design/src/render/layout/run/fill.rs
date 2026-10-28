//! A run's `"fill"` children: their share of the room the other children
//! leave, found by flexbox in a tree of the run's own — a proxy for each fill
//! child and one spacer as long as every other child and gap — so a change
//! costs the fill children and nothing beside them.

use super::*;

impl Renderer {
    /// The `"fill"` children's lengths under `a`: flexbox run on the fill
    /// children's proxies and one spacer as long as every other child and
    /// gap, as broad as the broadest of them — so the fill children share
    /// exactly the room the others leave, and the line's breadth is the
    /// broadest of everything. Each fill child is then laid out at its
    /// length.
    pub(super) fn flex(&mut self, runs: &mut Runs, run: &mut RunLayout, a: &mut Asked) {
        let n = run.children.len();
        let mut fills_along = 0.0f64;
        for k in run.fills.keys() {
            if let Some(s) = a.sizes.get(k) {
                fills_along += s.along as f64;
            }
        }
        let others = run.base.total().0 + a.sizes.total().0 - fills_along
            + run.gap as f64 * n.saturating_sub(1) as f64;
        let broad = run.broadest(a);
        let dim = |v: Option<f32>| v.map_or(Dimension::auto(), Dimension::length);
        let (row, align) = (run.row, run.align);
        let size = |along: Dimension, across: Dimension| {
            if row {
                Size {
                    width: along,
                    height: across,
                }
            } else {
                Size {
                    width: across,
                    height: along,
                }
            }
        };
        let spacer = Style {
            flex_shrink: 0.0,
            size: size(Dimension::length(others as f32), Dimension::length(broad)),
            ..Style::default()
        };
        run.flex.set_style(run.spacer, spacer).expect("style");
        let flexer = Style {
            flex_direction: if row {
                FlexDirection::Row
            } else {
                FlexDirection::Column
            },
            align_items: match align {
                Align::Center => AlignItems::CENTER,
                Align::End => AlignItems::FLEX_END,
                _ => AlignItems::FLEX_START,
            },
            size: size(dim(a.ask.main), dim(a.ask.line)),
            ..Style::default()
        };
        run.flex.set_style(run.flexer, flexer).expect("style");
        let along_room = match a.ask.main.or(a.ask.avail) {
            Some(m) => AvailableSpace::Definite(m),
            None if a.ask.least => AvailableSpace::MinContent,
            None => AvailableSpace::MaxContent,
        };
        let across_room = match a.ask.line {
            Some(l) => AvailableSpace::Definite(l),
            None => a.ask.room,
        };
        let available = if row {
            Size {
                width: along_room,
                height: across_room,
            }
        } else {
            Size {
                width: across_room,
                height: along_room,
            }
        };
        let mut flex = std::mem::replace(&mut run.flex, TaffyTree::new());
        let flexer_id = run.flexer;
        flex.compute_layout_with_measure(flexer_id, available, |input, _id, ctx, _style| {
            let Some(Some(Leaf::Proxy(k))) = ctx else {
                return taffy::LayoutOutput::from_outer_size(Size::ZERO);
            };
            let size = self.proxy(runs, run, k, input.known_dimensions, input.available_space);
            taffy::LayoutOutput::from_outer_size(size)
        })
        .expect("layout");
        let fl = flex.unrounded_layout(flexer_id).size;
        a.flexed = Some(if row { fl.width } else { fl.height });
        if a.ask.line.is_none() {
            a.line = if row { fl.height } else { fl.width };
        }
        let proxies: Vec<(OrderKey, NodeId)> =
            run.fills.iter().map(|(k, p)| (k.clone(), *p)).collect();
        let line = a.line;
        let mut sized = Vec::new();
        for (k, proxy) in &proxies {
            let l = flex.unrounded_layout(*proxy).size;
            let (along, own) = if row {
                (l.width, l.height)
            } else {
                (l.height, l.width)
            };
            sized.push((k.clone(), along, own));
        }
        run.flex = flex;
        for (k, along, own) in sized {
            let stretch = run.children[&k].stretch;
            let mut s = self.fill_child(runs, run, &k, a.ask, along, stretch.then_some(line));
            s.own = own;
            s.first = along;
            if a.sizes.get(&k) != Some(&s) {
                run.relaid.insert(k.clone());
            }
            a.sizes.insert(k, s);
        }
    }

    /// A `"fill"` child's size under what the run's flex tree asks of its
    /// proxy: the child laid out alone in its own container, given what the
    /// proxy is given.
    fn proxy(
        &mut self,
        runs: &mut Runs,
        run: &mut RunLayout,
        key: &OrderKey,
        known: Size<Option<f32>>,
        available: Size<AvailableSpace>,
    ) -> Size<f32> {
        let (wrap, node) = {
            let c = &run.children[key];
            (c.wrap, c.node)
        };
        let mut style = run.tree.style(wrap).expect("style").clone();
        let dim = |v: Option<f32>| v.map_or(Dimension::auto(), Dimension::length);
        style.size = Size {
            width: dim(known.width),
            height: dim(known.height),
        };
        run.tree.set_style(wrap, style).expect("style");
        self.settle_in(runs, &mut run.tree, wrap, available, &[]);
        run.children.get_mut(key).expect("a child").last = None;
        run.tree.unrounded_layout(node).size
    }

    /// A `"fill"` child laid out at `along`, its share of the free room,
    /// and across `line` when it stretches.
    pub(super) fn fill_child(
        &mut self,
        runs: &mut Runs,
        run: &mut RunLayout,
        key: &OrderKey,
        ask: Ask,
        along: f32,
        line: Option<f32>,
    ) -> Sized {
        let shape = Shape {
            room: ask.room,
            least: ask.least,
            line,
            basis: None,
            main: Some(along),
        };
        let (wrap, node, last) = {
            let c = &run.children[key];
            (c.wrap, c.node, c.last)
        };
        if last == Some(shape) {
            return self.sized(run, node);
        }
        let mut style = run.tree.style(wrap).expect("style").clone();
        let across = line.map_or(Dimension::auto(), Dimension::length);
        if run.row {
            style.size = Size {
                width: Dimension::length(along),
                height: across,
            };
        } else {
            style.size = Size {
                width: across,
                height: Dimension::length(along),
            };
        }
        run.tree.set_style(wrap, style).expect("style");
        let room = match line {
            Some(l) => AvailableSpace::Definite(l),
            None => ask.room,
        };
        let available = if run.row {
            Size {
                width: AvailableSpace::Definite(along),
                height: room,
            }
        } else {
            Size {
                width: room,
                height: AvailableSpace::Definite(along),
            }
        };
        self.settle_in(runs, &mut run.tree, wrap, available, &[]);
        run.children.get_mut(key).expect("a child").last = Some(shape);
        self.sized(run, node)
    }
}
