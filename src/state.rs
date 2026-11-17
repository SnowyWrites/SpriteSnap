use image::RgbaImage;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

pub type Slot = Rc<RefCell<Option<RgbaImage>>>;

#[derive(Clone)]
pub struct State {
    pub original: Option<RgbaImage>,
    pub source: Option<RgbaImage>,
    pub result: Option<RgbaImage>,
    pub recipe: Vec<Step>,
    pub block: f32,
    pub painted: bool,
}

#[derive(Default)]
pub struct History {
    pub undo: Vec<State>,
    pub redo: Vec<State>,
}

#[derive(Clone, Copy, Default)]
pub struct Drag {
    pub l: bool,
    pub r: bool,
    pub t: bool,
    pub b: bool,
    pub mv: bool,
}

#[derive(Default)]
pub struct Crop {
    pub active: bool,
    pub rect: (f32, f32, f32, f32), // x0, y0, x1, y1 in image pixels
    pub drag: Option<Drag>,
    pub start: (f32, f32),
    pub start_rect: (f32, f32, f32, f32),
    pub anchor: (f32, f32),
    pub grab: (f32, f32),
}

pub fn snapshot(
    original: &Slot,
    source: &Slot,
    result: &Slot,
    recipe: &Recipe,
    block: f32,
    painted: bool,
) -> State {
    State {
        original: original.borrow().clone(),
        source: source.borrow().clone(),
        result: result.borrow().clone(),
        recipe: recipe.borrow().clone(),
        block,
        painted,
    }
}

// Call BEFORE changing anything. Caps history at 20 and clears redo.
pub fn record(history: &Rc<RefCell<History>>, state: State) {
    let mut h = history.borrow_mut();
    h.undo.push(state);
    if h.undo.len() > 20 {
        h.undo.remove(0);
    }
    h.redo.clear();
}

// Put a snapshot back into the slots and redraw the preview.
pub fn apply(
    state: State,
    original: &Slot,
    source: &Slot,
    result: &Slot,
    recipe: &Recipe,
    block: &Cell<f32>,
    painted: &Cell<bool>,
) {
    *original.borrow_mut() = state.original;
    *source.borrow_mut() = state.source;
    *result.borrow_mut() = state.result;
    *recipe.borrow_mut() = state.recipe;
    block.set(state.block);
    painted.set(state.painted);
}

pub type CropSlot = Rc<RefCell<Crop>>;

#[derive(Clone)]
pub enum Step {
    Clean(f32),
    Map(String, f32, f32, u8),
    Smooth(usize, f32),
}
pub type Recipe = Rc<RefCell<Vec<Step>>>;
