//! Hit testing: a click answered from the index — the topmost chunk at the
//! point whose node has an interaction and whose clipping frames show it.

use kurbo::Rect as KRect;

use super::*;

impl Chunk {
    /// Where its node can be clicked, in the design's coordinates.
    pub fn hit(&self) -> Option<Hit> {
        let (action, area) = self.action.as_ref()?;
        let area = *area + self.origin;
        let area = self.cut.map_or(area, |c| area.intersect(c));
        Some(Hit {
            action: action.clone(),
            area,
        })
    }
}

impl List {
    /// The interaction of the topmost node at `(x, y)`, in the design's
    /// coordinates, that has one and whose clipping frames show it there.
    pub fn action_at(&self, x: f64, y: f64) -> Option<&str> {
        let point = KRect::new(x, y, x, y);
        self.found(point, 0.0).into_iter().rev().find_map(|order| {
            let chunk = &self.chunks[order];
            let (action, _) = chunk.action.as_ref()?;
            let a = chunk.hit()?.area;
            (x >= a.x0 && y >= a.y0 && x < a.x1 && y < a.y1).then_some(action.as_str())
        })
    }
}
