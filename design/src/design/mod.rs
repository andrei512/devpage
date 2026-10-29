//! A design as it is held while it is shown: its scene, how many nodes use
//! each of its files, and its drawing in each theme a caller asks for —
//! every one brought along by each change to the scene, the damage it does
//! to their tiles kept until it is taken. It answers what a canvas or a PNG
//! asks of a drawing: its size, the interaction at a point, the tiles a
//! view needs, and an area to paint — rounded first where an update left a
//! run's children a fraction of a pixel off, so what is painted there is a
//! fresh drawing's.
//!
//! **One rule for a scene that can't be drawn** — its images too many for
//! the GPU's atlas. The design keeps showing its last good drawing in that
//! theme: the drawing of the last scene that could be drawn, made afresh the
//! moment the scene stopped being drawable, which every tile, size and
//! click is answered from meanwhile — a tile no one painted before, after a
//! pan or a zoom, too — and which no damage reaches; why is
//! [`Design::problem`]. The drawing that follows the scene is kept beside
//! it, so the next change updates it at the change's cost, and the change
//! that makes the scene drawable again damages every tile of that theme.
//!
//! It holds no renderer — a server shares one between its designs, a canvas
//! has its own — and has no mode: the browser draws the theme it shows when
//! it is shown, the server every theme the design has, at once.

use kurbo::Rect as KRect;
use peniko::ImageData;

use crate::display::{Frame, List};
use crate::proto;
use crate::render::{Damage, Drawing, Renderer, Undrawable};
use crate::scene::{self, Applied, Edits, Scene, Theme, Uses};
use crate::tiles::{self, Level, View};

/// A design's scene — its one copy — the files it uses, and its drawing in
/// each theme it was asked to draw in.
pub struct Design {
    scene: Scene,
    uses: Uses,
    /// Light, then dark.
    themes: [Shown; 2],
    /// What changed since it was last taken, per theme.
    damage: Vec<(Theme, Damage)>,
}

/// One theme of a design.
#[derive(Default)]
struct Shown {
    /// The scene laid out and made into its display list in the theme;
    /// `None` before it is first asked for, or when it was let go.
    drawing: Option<Drawing>,
    /// A whole scene arrived since `drawing` was made: it is made afresh
    /// when next asked for, sharing its images' blobs where it paints the
    /// same ones.
    stale: bool,
    /// Why the scene can't be drawn in the theme. Nothing is painted from
    /// `drawing` meanwhile.
    broken: Option<Undrawable>,
    /// While the scene can't be drawn: the drawing of the last scene that
    /// could be, in the theme — what is shown meanwhile. `None` otherwise,
    /// or when no scene could be drawn in the theme yet.
    good: Option<Drawing>,
}

const THEMES: [Theme; 2] = [Theme::Light, Theme::Dark];

fn index(theme: Theme) -> usize {
    match theme {
        Theme::Light => 0,
        Theme::Dark => 1,
    }
}

impl Design {
    /// `scene`, drawn in no theme yet: every tile of both is stale.
    pub fn new(scene: Scene) -> Design {
        Design {
            uses: Uses::of(&scene.root),
            scene,
            themes: Default::default(),
            damage: THEMES.iter().map(|&t| (t, Damage::All)).collect(),
        }
    }

    /// The scene.
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The scene replaced whole — a watch stream sent all of it again. Every
    /// tile of both themes is stale, and each drawing is made afresh when
    /// next asked for; until one can be, the last good drawing in its theme
    /// is what is shown — rounded first where updates left it unrounded,
    /// while the scene it shows is still at hand.
    pub fn replace(&mut self, renderer: &mut Renderer, scene: Scene) {
        for shown in &mut self.themes {
            if shown.broken.is_some() {
                // The last good drawing stays; the one that followed the
                // scene before goes with it.
                shown.drawing = None;
            } else if let Some(drawing) = shown.drawing.as_mut().filter(|_| !shown.stale) {
                drawing.prepare(renderer, &self.scene, everywhere());
            }
            shown.stale = true;
            shown.broken = None;
        }
        self.uses = Uses::of(&scene.root);
        self.scene = scene;
        self.damage.extend(THEMES.iter().map(|&t| (t, Damage::All)));
    }

    /// Applies `edits` to the scene and brings every drawing along by what
    /// they did — at the cost of the edits, never the scene's — or says why
    /// they don't apply, leaving everything as it was. A theme whose scene
    /// they leave undrawable has its last good drawing made, from the scene
    /// before them: what it shows until a change makes the scene drawable
    /// again.
    pub fn apply(&mut self, renderer: &mut Renderer, edits: &Edits) -> Result<Applied, String> {
        let (applied, undo) = scene::applied(&mut self.scene, &mut self.uses, edits)?;
        let mut breaking = Vec::new();
        for theme in THEMES {
            let shown = &mut self.themes[index(theme)];
            let Some(drawing) = shown.drawing.as_mut().filter(|_| !shown.stale) else {
                // Made afresh, from this scene, when next asked for.
                shown.broken = None;
                continue;
            };
            match drawing.update(renderer, &self.scene, &applied) {
                Ok(damage) => {
                    // Back from a scene that couldn't be drawn: its tiles
                    // show the last one that could.
                    let damage = if shown.broken.take().is_some() {
                        shown.good = None;
                        Damage::All
                    } else {
                        damage
                    };
                    self.damage.push((theme, damage));
                }
                Err(undrawable) => {
                    if shown.broken.replace(undrawable).is_none() {
                        breaking.push(theme);
                    }
                }
            }
        }
        if !breaking.is_empty() {
            // The scene before the edits, which could be drawn, drawn afresh
            // in each theme they broke — the tiles painted already show it,
            // so none is damaged — and the edits applied to it again.
            undo.undo(&mut self.scene, &mut self.uses);
            for theme in breaking {
                self.themes[index(theme)].good = renderer.drawing(&self.scene, theme, None).ok();
            }
            scene::apply(&mut self.scene, &mut self.uses, edits)
                .expect("edits apply again to the scene they were just undone from");
        }
        Ok(applied)
    }

    /// Applies a watch stream's patch: [`Design::apply`] with the edits it
    /// carries, each image and font file the scene holds already taken from
    /// it.
    pub fn patch(&mut self, renderer: &mut Renderer, patch: &proto::Patch) -> Result<(), String> {
        let edits = crate::wire::edits(patch, &self.scene)?;
        self.apply(renderer, &edits).map(|_| ())
    }

    /// Draws the scene in `theme`, if it isn't: made afresh the first time,
    /// or after a whole scene — painting the last drawing's images, in
    /// either theme, when it paints the same ones, so the GPU keeps its
    /// atlas — or why it can't be drawn. A scene that couldn't be drawn
    /// isn't tried again until it changes.
    pub fn draw(&mut self, renderer: &mut Renderer, theme: Theme) -> Result<(), &Undrawable> {
        let i = index(theme);
        let shown = &self.themes[i];
        if shown.drawing.is_some() && !shown.stale || shown.broken.is_some() {
            return match &self.themes[i].broken {
                Some(undrawable) => Err(undrawable),
                None => Ok(()),
            };
        }
        let last = self.themes[i]
            .drawing
            .take()
            .or_else(|| self.themes[1 - i].drawing.take());
        let made = {
            let after = last.as_ref().or(self.themes[i].good.as_ref());
            renderer.drawing(&self.scene, theme, after)
        };
        // The other theme's drawing, taken to share its images' blobs, goes
        // back.
        let last = match last {
            Some(other) if other.theme() != theme => {
                self.themes[1 - i].drawing = Some(other);
                None
            }
            last => last,
        };
        let shown = &mut self.themes[i];
        match made {
            // Its tiles went stale when its scene arrived; any painted since
            // were the last good drawing's.
            Ok(drawing) => {
                shown.drawing = Some(drawing);
                shown.stale = false;
                if shown.good.take().is_some() {
                    self.damage.push((theme, Damage::All));
                }
                Ok(())
            }
            // The last drawing in the theme, of the scene before this one —
            // which could be drawn, or the last good one would be kept
            // already — is what is shown until a scene can be drawn again.
            Err(undrawable) => {
                if shown.good.is_none() {
                    shown.good = last;
                }
                Err(shown.broken.insert(undrawable))
            }
        }
    }

    /// Lets the drawing in `theme` go: the design isn't shown in it.
    pub fn forget(&mut self, theme: Theme) {
        self.themes[index(theme)] = Shown::default();
        self.damage.retain(|(t, _)| *t != theme);
    }

    /// Why the scene can't be drawn in `theme`, when it can't.
    pub fn problem(&self, theme: Theme) -> Option<&Undrawable> {
        self.themes[index(theme)].broken.as_ref()
    }

    /// The damage since it was last taken, per theme, in the order it was
    /// done.
    pub fn take_damage(&mut self) -> Vec<(Theme, Damage)> {
        std::mem::take(&mut self.damage)
    }

    /// The drawing shown in `theme`, drawn if it isn't — the last good one
    /// while the scene can't be drawn — or `None` when there is none to
    /// show: no scene could be drawn in it yet.
    pub fn drawing(&mut self, renderer: &mut Renderer, theme: Theme) -> Option<&Drawing> {
        let drawn = self.draw(renderer, theme).is_ok();
        let shown = &self.themes[index(theme)];
        if drawn {
            shown.drawing.as_ref()
        } else {
            shown.good.as_ref()
        }
    }

    /// The shown drawing's area in `theme`: its top-left and size, in the
    /// design's coordinates.
    pub fn area(&mut self, renderer: &mut Renderer, theme: Theme) -> Option<Frame> {
        self.drawing(renderer, theme).map(Drawing::area)
    }

    /// The interaction of the topmost node at `(x, y)` — in the design's
    /// coordinates — that has one, where its clipping frames show it.
    pub fn action_at(
        &mut self,
        renderer: &mut Renderer,
        theme: Theme,
        x: f64,
        y: f64,
    ) -> Option<&str> {
        let drawing = self.prepared(renderer, theme, KRect::new(x, y, x, y))?;
        drawing.action_at(x as f32, y as f32)
    }

    /// The tiles `view` needs at its own zoom's level, nearest its middle
    /// first — of the last good drawing while the scene can't be drawn.
    pub fn view_tiles(
        &mut self,
        renderer: &mut Renderer,
        theme: Theme,
        view: &View,
    ) -> Vec<(u32, u32)> {
        let Some(a) = self.area(renderer, theme) else {
            return Vec::new();
        };
        let area = KRect::new(
            a.x as f64,
            a.y as f64,
            (a.x + a.width) as f64,
            (a.y + a.height) as f64,
        );
        view.tiles(area, Level::of(view.zoom))
    }

    /// The images the shown drawing in `theme` paints, in the order it first
    /// paints them: what the GPU's atlas holds for it.
    pub fn images(&mut self, renderer: &mut Renderer, theme: Theme) -> Option<&[ImageData]> {
        self.drawing(renderer, theme).map(|d| d.images.as_slice())
    }

    /// The display list to paint tile `(col, row)` at `scale` from, in
    /// `theme` — the last good drawing's while the scene can't be drawn.
    pub fn tile(
        &mut self,
        renderer: &mut Renderer,
        theme: Theme,
        scale: f64,
        col: u32,
        row: u32,
    ) -> Option<&List> {
        self.painted(renderer, theme, |list| {
            tiles::tile_rect(list, scale, col, row)
        })
    }

    /// The display list to paint a strip of the drawing at 1× from, in
    /// `theme`: [`tiles::TILE`] pixels wide at tile column `col`, `rows`
    /// tall from pixel row `y` — a piece of a PNG band too wide for whole
    /// tiles.
    pub fn strip(
        &mut self,
        renderer: &mut Renderer,
        theme: Theme,
        col: u32,
        y: u32,
        rows: u32,
    ) -> Option<&List> {
        self.painted(renderer, theme, |list| {
            let (x0, y0) = (list.area.x as f64, list.area.y as f64);
            KRect::new(
                x0 + (col * tiles::TILE) as f64,
                y0 + y as f64,
                x0 + ((col + 1) * tiles::TILE) as f64,
                y0 + (y + rows) as f64,
            )
        })
    }

    /// The display list to paint the whole drawing from, in `theme`.
    pub fn whole(&mut self, renderer: &mut Renderer, theme: Theme) -> Option<&List> {
        self.painted(renderer, theme, |_| everywhere())
    }

    /// The shown drawing's display list in `theme`, rounded where `area` —
    /// in the design's coordinates, from the list — is painted.
    fn painted(
        &mut self,
        renderer: &mut Renderer,
        theme: Theme,
        area: impl FnOnce(&List) -> KRect,
    ) -> Option<&List> {
        let rect = area(&self.drawing(renderer, theme)?.list);
        self.prepared(renderer, theme, rect).map(|d| &d.list)
    }

    /// The shown drawing in `theme` with every run child in `rect` — in the
    /// design's coordinates — rounded where the runs now place it: what is
    /// read there is a fresh drawing's. A last good drawing needs none: it
    /// was made afresh, or rounded whole when its scene was replaced.
    fn prepared(&mut self, renderer: &mut Renderer, theme: Theme, rect: KRect) -> Option<&Drawing> {
        if self.draw(renderer, theme).is_err() {
            return self.themes[index(theme)].good.as_ref();
        }
        let drawing = self.themes[index(theme)].drawing.as_mut()?;
        drawing.prepare(renderer, &self.scene, rect);
        Some(drawing)
    }
}

/// An area holding all of any drawing.
fn everywhere() -> KRect {
    KRect::new(-1e12, -1e12, 1e12, 1e12)
}

#[cfg(test)]
impl Design {
    /// The drawing in `theme` as it is held — none made, nothing rounded —
    /// for a test to look inside.
    pub(crate) fn held(&mut self, theme: Theme) -> &mut Drawing {
        self.themes[index(theme)].drawing.as_mut().expect("drawn")
    }
}

#[cfg(test)]
mod tests;
