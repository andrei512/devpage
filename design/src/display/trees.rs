//! The property trees: transform, clip and effect nodes in arenas whose
//! slots are reused. A command names its clip and effect; its chunk names
//! its transform.

use kurbo::{BezPath, Vec2};

/// A node in one of the property trees, by its slot.
pub type Id = u32;

/// A scene node's place: its offset from its parent's.
#[derive(Clone, Debug)]
pub struct Transform {
    pub parent: Option<Id>,
    pub offset: Vec2,
}

/// A clipping shape, in the coordinates of its transform node.
#[derive(Clone)]
pub struct Clip {
    pub parent: Option<Id>,
    pub transform: Id,
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
    pub parent: Option<Id>,
    pub clip: Id,
    pub transform: Id,
    pub blend: Blend,
    pub bounds: Option<BezPath>,
}

/// A store of property-tree nodes whose slots are reused.
#[derive(Clone)]
pub struct Arena<T> {
    pub(super) slots: Vec<Option<T>>,
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

    pub fn get(&self, id: Id) -> &T {
        self.slots[id as usize].as_ref().expect("a live node")
    }

    pub fn get_mut(&mut self, id: Id) -> &mut T {
        self.slots[id as usize].as_mut().expect("a live node")
    }
}

/// The three property trees.
#[derive(Clone, Default)]
pub struct Trees {
    pub transforms: Arena<Transform>,
    pub clips: Arena<Clip>,
    pub effects: Arena<Effect>,
}
