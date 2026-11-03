//! The property trees: transform, clip and effect nodes in arenas whose
//! slots are reused. A command names its clip and effect; its chunk names
//! its transform.

use kurbo::{BezPath, Vec2};

/// A node in one of the property trees, by its slot.
pub type Id = u32;

/// A scene node's place: its offset from its parent's — or, for a child of
/// a [run](super::Run), the run's place for it, read from the run's prefix
/// sums whenever it is asked for, so a sibling's change of size moves it
/// without touching it.
#[derive(Clone, Debug)]
pub struct Transform {
    /// The parent node its offset is from; `None` at the root.
    pub parent: Option<Id>,
    /// Its offset from its parent's place.
    pub offset: Vec2,
    /// For a child of a run, the run and its key there, whose place is read
    /// from the run's prefix sums; `None` for any other node.
    pub item: Option<(super::RunId, crate::tree::OrderKey)>,
}

/// A clipping shape, in the coordinates of its transform node.
#[derive(Clone)]
pub struct Clip {
    /// The clip it lies inside; `None` at the root.
    pub parent: Option<Id>,
    /// The transform node whose coordinates `path` is in.
    pub transform: Id,
    /// Its shape; an empty one cuts nothing.
    pub path: BezPath,
}

/// How an effect's layer meets what is under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Blend {
    /// Drawn over it at an opacity.
    Opacity(f32),
    /// Added to it, color and alpha alike, so two fills in such layers that
    /// meet edge to edge sum to exactly what is in them, with no seam.
    Add,
}

/// A layer: its blend, the bound nothing in it reaches — in its transform's
/// coordinates, or, when `None`, the whole area being painted — and the clip
/// it is drawn inside.
#[derive(Clone)]
pub struct Effect {
    /// The layer it lies inside; `None` at the root.
    pub parent: Option<Id>,
    /// The clip it is drawn inside.
    pub clip: Id,
    /// The transform node whose coordinates `bounds` is in.
    pub transform: Id,
    /// How it meets what is under it.
    pub blend: Blend,
    /// The bound nothing in it reaches; `None` for the whole area being
    /// painted.
    pub bounds: Option<BezPath>,
}

/// A store of property-tree nodes whose slots are reused.
#[derive(Clone)]
pub struct Arena<T> {
    /// Each slot's node, by id; `None` for a free slot.
    pub(super) slots: Vec<Option<T>>,
    /// The free slots, the next to reuse last.
    pub(super) free: Vec<Id>,
}

impl<T> Default for Arena<T> {
    fn default() -> Self {
        Arena {
            slots: Vec::new(),
            free: Vec::new(),
        }
    }
}

impl<T> Arena<T> {
    /// Adds `t` in a free slot, or a new one: its id.
    pub fn add(&mut self, t: T) -> Id {
        match self.free.pop() {
            Some(id) => {
                self.slots[id as usize] = Some(t);
                id
            }
            None => {
                self.slots.push(Some(t));
                (self.slots.len() - 1) as Id
            }
        }
    }

    /// Removes node `id`, freeing its slot for reuse; nothing when it is not
    /// live.
    pub fn remove(&mut self, id: Id) {
        if self
            .slots
            .get_mut(id as usize)
            .and_then(Option::take)
            .is_some()
        {
            self.free.push(id);
        }
    }

    /// How many nodes are live.
    pub fn live(&self) -> usize {
        self.slots.len() - self.free.len()
    }

    /// Node `id`; panics when it is not live.
    pub fn get(&self, id: Id) -> &T {
        self.slots[id as usize].as_ref().expect("a live node")
    }

    /// Node `id`, to change; panics when it is not live.
    pub fn get_mut(&mut self, id: Id) -> &mut T {
        self.slots[id as usize].as_mut().expect("a live node")
    }
}

/// The three property trees.
#[derive(Clone, Default)]
pub struct Trees {
    /// Each scene node's place.
    pub transforms: Arena<Transform>,
    /// Every clipping shape, each in its transform node's coordinates.
    pub clips: Arena<Clip>,
    /// Every layer: its blend, its bound and the clip it is drawn inside.
    pub effects: Arena<Effect>,
}
