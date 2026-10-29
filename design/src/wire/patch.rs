//! Edits on the wire: [`Edits`] to the `proto::Patch` the watch stream sends
//! and the store logs, and back — each image and font file in it once.

use super::{decode, encode, encode_own, file, files, font_file, font_files, node_id, unnode_id};
use crate::proto::{self, op::Op as Kind};
use crate::scene::{self, Applied, Edit, Edits, Scene, Uses};

/// `edits` as a patch.
pub fn patch(edits: &Edits) -> proto::Patch {
    proto::Patch {
        ops: edits.list.iter().map(op).collect(),
        images: edits.images.values().map(|f| file(f)).collect(),
        fonts: edits.fonts.values().map(|f| font_file(f)).collect(),
        themed: edits.themed,
    }
}

fn op(edit: &Edit) -> proto::Op {
    let op = match edit {
        Edit::Change(n) => Kind::Change(encode_own(n)),
        Edit::Insert {
            parent,
            after,
            node,
        } => Kind::Insert(proto::Insert {
            parent: parent.as_ref().map(node_id),
            after: after.as_ref().map(node_id),
            node: Some(encode(node)),
        }),
        Edit::Remove(id) => Kind::Remove(node_id(id)),
        Edit::Move { id, parent, after } => Kind::Move(proto::Move {
            id: Some(node_id(id)),
            parent: Some(node_id(parent)),
            after: after.as_ref().map(node_id),
        }),
    };
    proto::Op { op: Some(op) }
}

/// The edits a patch to `scene` carries, or why it doesn't: each image and
/// font file the scene holds already is taken from it, not decoded or
/// checked again.
fn edits(patch: &proto::Patch, scene: &Scene) -> Result<Edits, String> {
    let list = patch
        .ops
        .iter()
        .map(|o| {
            Ok(match o.op.as_ref().ok_or("an empty operation")? {
                Kind::Change(n) => Edit::Change(decode(n)?),
                Kind::Insert(i) => Edit::Insert {
                    parent: i.parent.as_ref().map(|p| unnode_id(Some(p))).transpose()?,
                    after: i.after.as_ref().map(|a| unnode_id(Some(a))).transpose()?,
                    node: decode(i.node.as_ref().ok_or("an insert with no node")?)?,
                },
                Kind::Remove(id) => Edit::Remove(unnode_id(Some(id))?),
                Kind::Move(m) => Edit::Move {
                    id: unnode_id(m.id.as_ref())?,
                    parent: unnode_id(m.parent.as_ref())?,
                    after: m.after.as_ref().map(|a| unnode_id(Some(a))).transpose()?,
                },
            })
        })
        .collect::<Result<_, String>>()?;
    Ok(Edits {
        list,
        images: files(&patch.images, &scene.images)?,
        fonts: font_files(&patch.fonts, &scene.fonts)?,
        themed: patch.themed,
    })
}

/// Applies a patch to `scene`, whose files `uses` counts, as a browser
/// holding the scene does — or says why it can't, leaving both as they
/// were.
pub fn apply(scene: &mut Scene, uses: &mut Uses, patch: &proto::Patch) -> Result<Applied, String> {
    let edits = edits(patch, scene)?;
    scene::apply(scene, uses, &edits)
}

#[cfg(test)]
mod tests;
