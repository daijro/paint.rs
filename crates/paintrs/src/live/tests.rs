use super::*;
use pdn_core::testing::{Ev, drive_tool, fresh_doc};
use pdn_core::tool::{MouseButton, ToolType};
use std::cell::Cell;

thread_local! {
    /// A lower [`MAX_BYTES`] for this test thread ([`max_bytes`]).
    pub(super) static MAX_BYTES_HERE: Cell<Option<usize>> = const { Cell::new(None) };
    /// How many times [`entry`] began encoding an image's tiles on this test thread.
    pub(super) static ENCODES: Cell<u32> = const { Cell::new(0) };
}

/// A 300×200 image: a white Background and "Layer 2" over it, painted over its left part.
fn app() -> AppWorkspace {
    let mut app = fresh_doc(300, 200, None);
    let ws = app.active_ws_mut().unwrap();
    ws.add_layer(Layer::create_layer(300, 200, "Layer 2"));
    // Work in it: a new image doesn't replace it.
    ws.is_initial_workspace = false;
    fill(&mut app, 1, (0, 0, 100, 200), [20, 40, 60, 255]);
    app
}

fn fill(app: &mut AppWorkspace, layer: usize, (x, y, w, h): (u32, u32, u32, u32), px: [u8; 4]) {
    let ws = app.active_ws_mut().unwrap();
    let s = &mut ws.document.layers.get_at_mut(layer).unwrap().surface;
    for yy in y..y + h {
        for xx in x..x + w {
            s.set_pixel(xx, yy, px);
        }
    }
}

fn pixel(app: &AppWorkspace, layer: usize, x: u32, y: u32) -> [u8; 4] {
    app.active_ws()
        .unwrap()
        .document
        .layers
        .get_at(layer)
        .unwrap()
        .surface
        .pixel(x, y)
}

/// The checkpoint of the active image as image 1 of a set.
fn bytes(app: &AppWorkspace) -> Vec<u8> {
    let e = entry(&mut HashMap::new(), app.active_ws().unwrap(), 1).unwrap();
    write(&set([&e].into_iter())).unwrap()
}

/// The bytes two checkpoints differ in, as the shell's delta sees them (between their common prefix and suffix).
fn delta(a: &[u8], b: &[u8]) -> usize {
    let prefix = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let suffix = a[prefix..]
        .iter()
        .rev()
        .zip(b[prefix..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    b.len() - prefix - suffix
}

/// The set's image 1 built as a document.
fn image(bytes: &[u8]) -> Document {
    let parts = read(bytes).unwrap();
    let (uid, d) = documents(&parts.doc).next().unwrap();
    let plan = prepare(&parts, uid, d, None, &HashMap::new()).unwrap();
    assemble(plan, None, &mut HashMap::new())
}

fn copy(app: &AppWorkspace) -> AppWorkspace {
    let mut other = fresh_doc(300, 200, None);
    let doc = image(&bytes(app));
    other
        .active_ws_mut()
        .unwrap()
        .set_document_and_active_layer(doc, 1);
    other
}

#[test]
fn checkpoints_are_canonical_local_and_leave_views_out() {
    let mut app = app();
    let a = bytes(&app);
    pdn_core::action::perform(&mut app, "menu.edit.selectAll");
    let ws = app.active_ws_mut().unwrap();
    ws.active_layer = 0;
    ws.view.zoom = 3.0;
    assert_eq!(
        bytes(&app),
        a,
        "the selection, the active layer and the view are not shared"
    );
    assert_eq!(
        bytes(&copy(&app)),
        a,
        "a checkpoint taken in encodes to the same bytes"
    );
    assert_eq!(
        image(&a).layers.get_at(1).unwrap().surface.pixel(5, 5),
        [20, 40, 60, 255]
    );
    // A stroke within one tile of the top layer changes about that tile's bytes only.
    fill(&mut app, 1, (150, 100, 20, 4), [255, 0, 0, 255]);
    let b = bytes(&app);
    let changed = delta(&a, &b);
    assert!(changed < 512, "{changed} of {} bytes", b.len());
    assert!(read(&b[..b.len() - 1]).is_err() && read(&[b.as_slice(), &[0]].concat()).is_err());
    assert!(read(&vec![0; MAX_BYTES + 1]).is_err_and(|e| e.contains("over the 128 MiB limit")));
}

#[test]
fn merges_layers_fields_and_tiles_and_both_sides_pixels_in_one_tile() {
    let base = app();
    let (mut left, mut right) = (copy(&base), copy(&base));
    fill(&mut left, 1, (10, 10, 20, 20), [255, 0, 0, 255]);
    fill(&mut right, 0, (200, 150, 20, 20), [0, 0, 255, 255]);
    right
        .active_ws_mut()
        .unwrap()
        .document
        .layers
        .get_at_mut(1)
        .unwrap()
        .properties
        .opacity = 128;
    // The same tile on both sides, different pixels: both stay; a pixel both painted takes the side not yet accepted.
    fill(&mut left, 0, (130, 10, 4, 4), [1, 1, 1, 255]);
    fill(&mut right, 0, (140, 10, 4, 4), [2, 2, 2, 255]);
    fill(&mut left, 0, (150, 10, 1, 1), [3, 3, 3, 255]);
    fill(&mut right, 0, (150, 10, 1, 1), [4, 4, 4, 255]);
    let (merged, _) = merge(&bytes(&base), &bytes(&left), &bytes(&right)).unwrap();
    let out = fresh_doc(300, 200, None);
    let mut out = out;
    out.active_ws_mut()
        .unwrap()
        .set_document_and_active_layer(image(&merged), 0);
    assert_eq!(pixel(&out, 1, 15, 15), [255, 0, 0, 255]);
    assert_eq!(pixel(&out, 0, 205, 155), [0, 0, 255, 255]);
    assert_eq!(
        out.active_ws()
            .unwrap()
            .document
            .layers
            .get_at(1)
            .unwrap()
            .properties
            .opacity,
        128
    );
    assert_eq!(
        (
            pixel(&out, 0, 131, 11),
            pixel(&out, 0, 141, 11),
            pixel(&out, 0, 150, 10)
        ),
        ([1, 1, 1, 255], [2, 2, 2, 255], [3, 3, 3, 255])
    );
    assert_eq!(bytes(&out), merged, "the merge is canonical");
    // The same layer property on both sides is a conflict, even to the same value.
    let mut faded = copy(&base);
    faded
        .active_ws_mut()
        .unwrap()
        .document
        .layers
        .get_at_mut(1)
        .unwrap()
        .properties
        .opacity = 128;
    assert!(
        merge(&bytes(&base), &bytes(&faded), &bytes(&right))
            .err()
            .unwrap()
            .contains("Conflict")
    );
}

#[test]
fn a_text_layers_render_both_sides_changed_is_a_conflict() {
    let mut base = app();
    base.active_ws_mut()
        .unwrap()
        .document
        .layers
        .get_at_mut(1)
        .unwrap()
        .text = Some(TextSpec::default());
    let (mut left, mut right, mut faded) = (copy(&base), copy(&base), copy(&base));
    fill(&mut left, 1, (10, 10, 4, 4), [255, 0, 0, 255]);
    fill(&mut right, 1, (200, 10, 4, 4), [0, 0, 255, 255]);
    faded
        .active_ws_mut()
        .unwrap()
        .document
        .layers
        .get_at_mut(1)
        .unwrap()
        .properties
        .opacity = 10;
    assert!(
        merge(&bytes(&base), &bytes(&left), &bytes(&right))
            .err()
            .unwrap()
            .contains("text")
    );
    assert!(merge(&bytes(&base), &bytes(&left), &bytes(&faded)).is_ok());
}

#[test]
fn a_new_canvas_size_conflicts_with_paint_and_a_deleted_layer_with_its_paint() {
    let base = app();
    let (mut painted, mut cropped, mut deleted) = (copy(&base), copy(&base), copy(&base));
    fill(&mut painted, 1, (10, 10, 4, 4), [255, 0, 0, 255]);
    let mut small = Document::new(100, 100, 96.0);
    for l in image(&bytes(&base)).layers.iter() {
        let mut l = l.clone();
        l.surface = pdn_core::document::surface::Surface::create(100, 100);
        small.add_layer(l);
    }
    cropped
        .active_ws_mut()
        .unwrap()
        .set_document_and_active_layer(small, 0);
    assert!(
        merge(&bytes(&base), &bytes(&painted), &bytes(&cropped))
            .err()
            .unwrap()
            .contains("canvas size")
    );
    deleted.active_ws_mut().unwrap().document.remove_layer_at(1);
    assert!(
        merge(&bytes(&base), &bytes(&painted), &bytes(&deleted))
            .err()
            .unwrap()
            .contains("Conflict")
    );
    // Untouched, it goes.
    let (merged, _) = merge(&bytes(&base), &bytes(&base), &bytes(&deleted)).unwrap();
    assert_eq!(image(&merged).layers.size(), 1);
}

#[test]
fn concurrent_layer_additions_reach_the_same_bytes_on_the_coordinator_and_every_peer() {
    let base = app();
    let add = |name: &str| {
        let mut a = copy(&base);
        a.active_ws_mut()
            .unwrap()
            .add_layer(Layer::create_layer(300, 200, name));
        bytes(&a)
    };
    let (base, first, second) = (bytes(&base), add("first"), add("second"));
    // Accepted first: `first`. The coordinator rebases `second` over it with its author's calls.
    let coordinator = merge(&base, &merge(&base, &base, &second).unwrap().0, &first)
        .unwrap()
        .0;
    let second_author = merge(&base, &second, &first).unwrap().0;
    let first_author = merge(&first, &first, &coordinator).unwrap().0;
    assert_eq!(
        (&coordinator, &first_author),
        (&second_author, &second_author)
    );
    let names: Vec<_> = image(&coordinator)
        .layers
        .iter()
        .map(|l| l.properties.name.clone())
        .collect();
    assert_eq!(names, ["Background", "Layer 2", "second", "first"]);
}

/// Connects a fresh session over `app` and takes its first checkpoint.
fn session(app: &mut AppWorkspace) -> Vec<u8> {
    connect();
    step(app, true, None);
    checkpoint(app).unwrap().to_vec()
}

/// `edit` applied to `app`'s set as someone else's change, queued with `base`.
fn theirs(app: &AppWorkspace, edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut parts = read(&checkpoint(app).unwrap()).unwrap();
    edit(&mut parts.doc);
    write(&parts).unwrap()
}

#[test]
fn remote_changes_wait_for_a_quiet_frame_keep_views_and_undo_goes_to_the_session() {
    let mut app = app();
    pdn_core::action::perform(&mut app, "menu.edit.selectAll");
    app.active_ws_mut().unwrap().active_layer = 1;
    let base = session(&mut app);
    let edited = theirs(&app, |doc| {
        doc["documents"][0]["layers"][0]["visible"] = false.into();
        doc["documents"][0]["layers"][1]["opacity"] = 99.into();
    });
    let done = Rc::new(std::cell::RefCell::new(None));
    let to = done.clone();
    queue(
        Some(base),
        edited,
        Box::new(move |r| *to.borrow_mut() = Some(r)),
    );
    step(&mut app, false, None);
    assert!(done.borrow().is_none(), "a busy app waits");
    assert_eq!(
        checkpoint(&app),
        Err("busy".into()),
        "no half-done gesture is read"
    );
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    let ws = app.active_ws().unwrap();
    assert!(!ws.document.layers.get_at(0).unwrap().properties.visible);
    assert_eq!(ws.document.layers.get_at(1).unwrap().properties.opacity, 99);
    assert_eq!(ws.active_layer, 1, "the active layer is this person's");
    assert!(!ws.selection.is_empty(), "so is the selection");
    assert_eq!(
        ws.history.undo.len(),
        1,
        "no native step to undo it, nor older ones without it"
    );
    let changes = LIVE.with_borrow(|live| live.changes);
    step(&mut app, true, None);
    assert_eq!(
        LIVE.with_borrow(|live| live.changes),
        changes,
        "a merged remote change is not reported back"
    );
    // A selection changes nothing shared.
    pdn_core::action::perform(&mut app, "menu.edit.deselect");
    step(&mut app, true, None);
    assert_eq!(LIVE.with_borrow(|live| live.changes), changes);
    // Paint does.
    fill(&mut app, 1, (10, 10, 4, 4), [9, 9, 9, 255]);
    step(&mut app, true, None);
    assert_eq!(LIVE.with_borrow(|live| live.changes), changes + 1);
    // Undo and Redo go to the session.
    assert!(pdn_core::action::is_executable(&app, "menu.edit.undo"));
    pdn_core::action::perform(&mut app, "menu.edit.undo");
    pdn_core::action::perform(&mut app, "menu.edit.redo");
    assert_eq!(LIVE.with_borrow(|live| live.undos.clone()), [false, true]);
    assert!(!app.step_to(0), "no History steps while connected");
    disconnect();
    assert!(
        !pdn_core::live::takes_undo(&app, false, false),
        "native Undo once disconnected"
    );
}

#[test]
fn opened_images_join_the_set_theirs_open_in_the_background_and_closed_ones_stay_closed() {
    let mut app = app();
    session(&mut app);
    app.create_blank_document_in_new_workspace(64, 64, 96.0, false);
    step(&mut app, true, None);
    let two = checkpoint(&app).unwrap().to_vec();
    let mut parts = read(&two).unwrap();
    assert_eq!(
        documents(&parts.doc).count(),
        2,
        "an image opened while connected joins the set"
    );
    // Someone else opens a third: it arrives as a tab, the image shown stays.
    let mut third = parts.doc["documents"][1].clone();
    let uid = 1u64 << 51;
    third["id"] = uid.into();
    third["name"] = "theirs.png".into();
    for (lid, _) in layers(&third).collect::<Vec<_>>() {
        let tiles = parts.tiles[&(documents(&parts.doc).nth(1).unwrap().0, lid)].clone();
        parts.tiles.insert((uid, lid), tiles);
    }
    parts.doc["documents"]
        .as_array_mut()
        .unwrap()
        .push(third.clone());
    parts.doc["documents"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(|d| d["id"].as_u64());
    let theirs = write(&parts).unwrap();
    queue(
        Some(two),
        theirs.clone(),
        Box::new(|r| assert!(r.is_ok(), "{r:?}")),
    );
    step(&mut app, true, None);
    let names = |app: &AppWorkspace| {
        app.documents
            .iter()
            .map(|ws| ws.file_name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&app), [None, None, Some("theirs.png".into())]);
    assert_eq!(app.active, Some(1));
    // Closing it hides it here only: the set keeps it, and a change to it doesn't open it again.
    app.close_document_workspace(2);
    step(&mut app, true, None);
    assert_eq!(checkpoint(&app).unwrap().as_slice(), theirs.as_slice());
    let renamed = {
        let mut parts = read(&theirs).unwrap();
        for d in parts.doc["documents"].as_array_mut().unwrap() {
            if d["id"] == uid {
                d["name"] = "renamed.png".into();
            }
        }
        write(&parts).unwrap()
    };
    queue(
        Some(theirs),
        renamed,
        Box::new(|r| assert!(r.is_ok(), "{r:?}")),
    );
    step(&mut app, true, None);
    assert_eq!(app.documents.len(), 2);
    let set = read(&checkpoint(&app).unwrap()).unwrap();
    assert_eq!(
        documents(&set.doc).find(|(id, _)| *id == uid).unwrap().1["name"],
        "renamed.png"
    );
    disconnect();
}

#[test]
fn presence_is_the_shared_image_its_active_layer_selection_bounds_and_the_gesture() {
    let mut app = app();
    session(&mut app);
    pdn_core::action::perform(&mut app, "menu.edit.selectAll");
    step(&mut app, true, Some([12.25, 30.0]));
    let (uid, layer) = LIVE.with_borrow(|live| {
        (
            live.docs[0].uid,
            app.active_ws()
                .unwrap()
                .document
                .layers
                .get_at(1)
                .unwrap()
                .id,
        )
    });
    assert_eq!(
        LIVE.with_borrow(|live| live.presence.clone()),
        json!({"doc": uid, "pointer": [12.3, 30], "layers": [layer], "selection": [0, 0, 300, 200]})
    );
    // A brush stroke under way: the others are shown it so far, at the brush's size.
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[
            Ev::Down(20.0, 20.0, MouseButton::Left),
            Ev::Move(40.0, 30.0),
        ],
    );
    step(&mut app, false, None);
    let stroke = LIVE.with_borrow(|live| live.presence["stroke"].clone());
    assert!(stroke["size"].as_f64().is_some_and(|s| s > 0.0), "{stroke}");
    assert_eq!(
        stroke["points"]
            .as_array()
            .map(|p| (p.first().cloned(), p.last().cloned())),
        Some((Some(json!(20)), Some(json!(30))))
    );
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Up(40.0, 30.0, MouseButton::Left)],
    );
    // A marquee being dragged.
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[
            Ev::Down(10.0, 10.0, MouseButton::Left),
            Ev::Move(50.0, 40.0),
        ],
    );
    step(&mut app, false, None);
    assert_eq!(
        LIVE.with_borrow(|live| live.presence["band"].clone()),
        json!([10, 10, 51, 41]),
        "the pixels it would select"
    );
    drive_tool(
        &mut app,
        ToolType::RectangleSelect,
        &[Ev::Up(50.0, 40.0, MouseButton::Left)],
    );
    disconnect();
}

/// `edit` applied to the set `bytes` as someone else's change.
fn theirs_of(bytes: &[u8], edit: impl FnOnce(&mut Value)) -> Vec<u8> {
    let mut parts = read(bytes).unwrap();
    edit(&mut parts.doc);
    write(&parts).unwrap()
}

/// What `queue` settles with, once a frame takes it.
type Settled = Rc<std::cell::RefCell<Option<Result<Option<Vec<u8>>, String>>>>;

fn queued(base: Option<Vec<u8>>, edited: Vec<u8>) -> Settled {
    let done: Settled = Rc::default();
    let to = done.clone();
    queue(base, edited, Box::new(move |r| *to.borrow_mut() = Some(r)));
    done
}

/// A set of one 32×32 image, someone else's.
fn other_set() -> Vec<u8> {
    bytes(&fresh_doc(32, 32, Some(3)))
}

#[test]
fn adopting_a_whole_set_gives_back_the_images_it_replaced_for_a_draft() {
    let mut app = app();
    let mine = session(&mut app);
    let done = queued(None, other_set());
    step(&mut app, true, None);
    assert_eq!(
        *done.borrow(),
        Some(Ok(Some(mine))),
        "the shell keeps it as a draft"
    );
    let sizes: Vec<_> = (app.documents.iter())
        .map(|ws| (ws.document.width, ws.document.height))
        .collect();
    assert_eq!(sizes, [(32, 32)]);
    disconnect();
}

/// Opens a 4×4 image over the layer limit as a new tab: it can't be shared.
fn open_over_the_limit(app: &mut AppWorkspace) -> u64 {
    app.create_blank_document_in_new_workspace(4, 4, 96.0, false);
    let ws = app.active_ws_mut().unwrap();
    for i in 0..MAX_LAYERS {
        ws.document
            .add_layer(Layer::create_layer(4, 4, &format!("Layer {i}")));
    }
    ws.id
}

/// The images of the set as this person's checkpoint has them, by width.
fn widths(bytes: &[u8]) -> Vec<u64> {
    let parts = read(bytes).unwrap();
    documents(&parts.doc)
        .map(|(_, d)| d["width"].as_u64().unwrap())
        .collect()
}

/// The set's id of the image workspace `id` shows.
fn uid_here(_app: &AppWorkspace, id: u64) -> u64 {
    LIVE.with_borrow(|live| live.docs.iter().find(|s| s.local == Some(id)).unwrap().uid)
}

/// The workspace with this id's image.
fn ws(app: &AppWorkspace, id: u64) -> &DocumentWorkspace {
    app.documents.iter().find(|ws| ws.id == id).unwrap()
}

#[test]
fn an_image_over_a_limit_stays_alone_and_unsent_work_survives_every_adopt() {
    let mut app = app();
    let shared = app.active_ws().unwrap().id;
    let base = session(&mut app);
    let over = open_over_the_limit(&mut app);
    step(&mut app, true, None);
    // The checkpoint is the images that fit: the one over the limit stays here alone, and says why.
    assert_eq!(checkpoint(&app).unwrap().as_slice(), base.as_slice());
    let why = LIVE.with_borrow(|live| live.alone.iter().find(|a| a.local == over).map(|a| a.why));
    assert_eq!(why, Some(Why::Layers));
    // Unsent paint on the shared image, then someone else's change: it merges in.
    app.set_active_document_workspace(app.documents.iter().position(|w| w.id == shared));
    fill(&mut app, 1, (205, 105, 4, 4), [7, 7, 7, 255]);
    let theirs = theirs_of(&base, |doc| {
        doc["documents"][0]["layers"][0]["opacity"] = 9.into()
    });
    let done = queued(Some(base.clone()), theirs.clone());
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    assert_eq!(
        pixel(&app, 1, 205, 105),
        [7, 7, 7, 255],
        "the unsent paint stays"
    );
    let opacity = ws(&app, shared)
        .document
        .layers
        .get_at(0)
        .unwrap()
        .properties
        .opacity;
    assert_eq!(opacity, 9);
    // A whole set (a rejected proposal's restore, a resync) gives back what it replaces, paint and all, as a draft.
    let mine = checkpoint(&app).unwrap().to_vec();
    let done = queued(None, theirs);
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(Some(mine))));
    assert_eq!(
        ws(&app, over).document.layers.size(),
        MAX_LAYERS + 1,
        "never touched"
    );
    // Native Undo for the image over the limit.
    app.set_active_document_workspace(app.documents.iter().position(|w| w.id == over));
    assert!(!pdn_core::live::takes_undo(&app, false, false));
    app.set_active_document_workspace(app.documents.iter().position(|w| w.id == shared));
    assert!(pdn_core::live::takes_undo(&app, false, false));
    disconnect();
}

#[test]
fn joining_drafts_the_shareable_images_and_never_shares_the_one_kept_open() {
    let mut app = app();
    connect();
    let over = open_over_the_limit(&mut app);
    step(&mut app, true, None);
    let mine = checkpoint(&app).unwrap().to_vec();
    assert_eq!(widths(&mine), [300]);
    let done = queued(None, other_set());
    step(&mut app, true, None);
    assert_eq!(
        *done.borrow(),
        Some(Ok(Some(mine))),
        "the private image as a draft"
    );
    let open: Vec<_> = app.documents.iter().map(|w| w.document.width).collect();
    assert_eq!(open, [4, 32], "the one over the limit beside the session's");
    // Closed, it never reaches the session.
    let i = app.documents.iter().position(|w| w.id == over).unwrap();
    app.close_document_workspace(i);
    step(&mut app, true, None);
    assert_eq!(widths(&checkpoint(&app).unwrap()), [32]);
    disconnect();
}

#[test]
fn a_shared_image_changed_past_a_limit_leaves_the_set_as_it_was() {
    let mut app = app();
    let base = session(&mut app);
    let id = app.active_ws().unwrap().id;
    let ws = app.active_ws_mut().unwrap();
    for i in 0..MAX_LAYERS {
        ws.document
            .add_layer(Layer::create_layer(300, 200, &format!("Layer {i}")));
    }
    step(&mut app, true, None);
    assert_eq!(
        checkpoint(&app).unwrap().as_slice(),
        base.as_slice(),
        "the version from before"
    );
    let why = LIVE.with_borrow(|live| live.alone.iter().find(|a| a.local == id).map(|a| a.why));
    assert_eq!(why, Some(Why::Left));
    // Someone else's change to the set's version doesn't touch the image here.
    let theirs = theirs_of(&base, |doc| {
        doc["documents"][0]["layers"][0]["opacity"] = 9.into()
    });
    let done = queued(Some(base), theirs);
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    assert_eq!(app.documents.len(), 1);
    assert_eq!(
        app.active_ws().unwrap().document.layers.size(),
        MAX_LAYERS + 2
    );
    // Back within the limit, it stays out: the set's version changed meanwhile.
    while app.active_ws().unwrap().document.layers.size() > 2 {
        app.active_ws_mut().unwrap().document.remove_layer_at(2);
    }
    step(&mut app, true, None);
    let why = LIVE.with_borrow(|live| live.alone.iter().find(|a| a.local == id).map(|a| a.why));
    assert_eq!(why, Some(Why::Left));
    disconnect();
}

#[test]
fn a_shared_image_back_within_the_limits_rejoins_when_no_one_changed_it_meanwhile() {
    let mut app = app();
    session(&mut app);
    let id = app.active_ws().unwrap().id;
    let uid = uid_here(&app, id);
    for i in 0..MAX_LAYERS {
        let layer = Layer::create_layer(300, 200, &format!("Layer {i}"));
        app.active_ws_mut().unwrap().document.add_layer(layer);
    }
    step(&mut app, true, None);
    assert!(LIVE.with_borrow(|live| live.alone.iter().any(|a| a.local == id)));
    while app.active_ws().unwrap().document.layers.size() > 2 {
        app.active_ws_mut().unwrap().document.remove_layer_at(2);
    }
    fill(&mut app, 1, (1, 1, 1, 1), [3, 3, 3, 255]);
    step(&mut app, true, None);
    assert!(LIVE.with_borrow(|live| live.alone.is_empty()));
    assert_eq!(uid_here(&app, id), uid, "the same image of the set");
    let set = read(&checkpoint(&app).unwrap()).unwrap();
    assert_eq!(
        image(&write(&set).unwrap())
            .layers
            .get_at(1)
            .unwrap()
            .surface
            .pixel(1, 1),
        [3, 3, 3, 255]
    );
    disconnect();
}

#[test]
fn closing_an_image_this_person_opened_and_no_one_changed_takes_it_out_of_the_set() {
    let mut app = app();
    session(&mut app);
    for _ in 0..3 {
        app.create_blank_document_in_new_workspace(16, 16, 96.0, false);
    }
    step(&mut app, true, None);
    assert_eq!(widths(&checkpoint(&app).unwrap()).len(), 4);
    // Someone else changes the second tab's image: closing it keeps it in the set (theirs to close). Picked by its
    // tab, not by id order (the ids are random).
    let before = checkpoint(&app).unwrap().to_vec();
    let second = uid_here(&app, app.documents[1].id);
    let theirs = theirs_of(&before, |doc| {
        for d in doc["documents"].as_array_mut().unwrap() {
            if d["id"] == second {
                d["layers"][0]["opacity"] = 9.into();
            }
        }
    });
    let done = queued(Some(before), theirs);
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    while app.documents.len() > 1 {
        app.close_document_workspace(app.documents.len() - 1);
    }
    step(&mut app, true, None);
    let left = read(&checkpoint(&app).unwrap()).unwrap();
    let ids: Vec<_> = documents(&left.doc).map(|(uid, _)| uid).collect();
    assert_eq!(ids.len(), 2, "the one shown and the one someone changed");
    assert!(ids.contains(&second));
    disconnect();
}

#[test]
fn a_full_session_keeps_a_new_image_alone_until_closing_one_makes_room() {
    let mut app = fresh_doc(8, 8, None);
    // Work in it: a new image doesn't replace it.
    app.active_ws_mut().unwrap().is_initial_workspace = false;
    connect();
    for _ in 1..MAX_IMAGES {
        app.create_blank_document_in_new_workspace(8, 8, 96.0, false);
    }
    step(&mut app, true, None);
    let extra = {
        app.create_blank_document_in_new_workspace(8, 8, 96.0, false);
        app.active_ws().unwrap().id
    };
    step(&mut app, true, None);
    assert_eq!(widths(&checkpoint(&app).unwrap()).len(), MAX_IMAGES);
    let why = LIVE.with_borrow(|live| live.alone.iter().find(|a| a.local == extra).map(|a| a.why));
    assert_eq!(why, Some(Why::Full));
    // Closing one this person opened makes room: the new one joins at its next change.
    app.close_document_workspace(0);
    step(&mut app, true, None);
    fill(&mut app, 0, (0, 0, 1, 1), [1, 2, 3, 255]);
    step(&mut app, true, None);
    assert_eq!(widths(&checkpoint(&app).unwrap()).len(), MAX_IMAGES);
    assert!(LIVE.with_borrow(|live| live.alone.is_empty()));
    disconnect();
}

#[test]
fn undo_keeps_native_undo_through_perspective_warp_and_a_tool_parked_in_another_tab() {
    let mut app = app();
    session(&mut app);
    assert!(pdn_core::tool::perspective_warp::begin(&mut app));
    assert!(
        !pdn_core::live::takes_undo(&app, false, false),
        "Perspective Warp"
    );
    pdn_core::tool::perspective_warp::end(&mut app, false);
    assert!(pdn_core::live::takes_undo(&app, false, false));
    // A shape still being edited, parked when another image shows.
    drive_tool(
        &mut app,
        ToolType::Shapes,
        &[
            Ev::Down(20.0, 20.0, MouseButton::Left),
            Ev::Move(60.0, 50.0),
            Ev::Up(60.0, 50.0, MouseButton::Left),
        ],
    );
    app.create_blank_document_in_new_workspace(16, 16, 96.0, false);
    assert!(app.documents.iter().any(|ws| ws.parked_tool.is_some()));
    assert!(
        !pdn_core::live::takes_undo(&app, false, false),
        "a parked tool"
    );
    disconnect();
}

#[test]
fn a_set_in_another_shape_than_this_build_writes_is_refused() {
    let refused = |bytes: Vec<u8>, what: &str| assert!(read(&bytes).is_err(), "{what}");
    refused(
        edited(|p| p.doc["documents"][0]["junk"] = "x".repeat(1000).into()),
        "an unknown image field",
    );
    refused(
        edited(|p| p.doc["documents"][0]["layers"][0]["junk"] = 1.into()),
        "an unknown layer field",
    );
    refused(
        edited(|p| p.doc["documents"][0]["layers"][0]["text"] = json!({"text": "a"})),
        "a text layer missing its other fields",
    );
    let mut spec = serde_json::to_value(TextSpec::default()).unwrap();
    spec["extra"] = 1.into();
    refused(
        edited(|p| p.doc["documents"][0]["layers"][0]["text"] = spec),
        "a text layer with a field this build doesn't have",
    );
    let spec = serde_json::to_value(TextSpec::default()).unwrap();
    assert!(
        read(&edited(
            |p| p.doc["documents"][0]["layers"][0]["text"] = spec
        ))
        .is_ok()
    );
}

#[test]
fn taking_in_a_change_patches_only_its_tiles_into_the_layer_kept() {
    let mut app = app();
    let base = session(&mut app);
    let before = app
        .active_ws()
        .unwrap()
        .document
        .layers
        .get_at(1)
        .unwrap()
        .surface
        .revision();
    // Someone paints one tile of "Layer 2": the layer here stays (patched), every other pixel as it was.
    let mut other = copy(&app);
    fill(&mut other, 1, (130, 70, 4, 4), [9, 9, 9, 255]);
    let mut parts = read(&bytes(&other)).unwrap();
    let uid = documents(&read(&base).unwrap().doc).next().unwrap().0;
    parts.doc["documents"][0]["id"] = uid.into();
    let tiles: Vec<_> = parts
        .tiles
        .drain()
        .map(|((_, lid), t)| ((uid, lid), t))
        .collect();
    parts.tiles.extend(tiles);
    let done = queued(Some(base), write(&parts).unwrap());
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    let layer = app.active_ws().unwrap().document.layers.get_at(1).unwrap();
    assert_ne!(layer.surface.revision(), before, "patched");
    assert_eq!(pixel(&app, 1, 131, 71), [9, 9, 9, 255]);
    assert_eq!(pixel(&app, 1, 5, 5), [20, 40, 60, 255]);
    disconnect();
}

/// `edit` of a one-image set, written out (as a peer could send it).
fn edited(edit: impl FnOnce(&mut Parts)) -> Vec<u8> {
    let mut parts = read(&bytes(&app())).unwrap();
    edit(&mut parts);
    write(&parts).unwrap()
}

/// A set of `images` w×h images of `layers` blank layers each, ids from 1.
fn blank_set(images: u64, layers: u64, (w, h): (u32, u32)) -> Vec<u8> {
    let tiles = Tiles::from(vec![
        Rc::<[u8]>::from(&[0u8, 0, 0, 0, 0][..]);
        tile_count(w, h)
    ]);
    let mut parts = Parts::default();
    let docs: Vec<Value> = (1..=images)
        .map(|uid| {
            let list: Vec<Value> = (1..=layers)
                .map(|lid| {
                    parts.tiles.insert((uid, lid), tiles.clone());
                    json!({"id": lid, "name": "", "visible": true, "background": false, "opacity": 255,
                        "blend": "normal", "text": null, "tiles": []})
                })
                .collect();
            json!({"id": uid, "name": null, "width": w, "height": h, "resolution": 96.0, "layers": list})
        })
        .collect();
    parts.doc = json!({ "documents": docs });
    write(&parts).unwrap()
}

#[test]
fn a_set_over_the_limits_or_malformed_is_refused_before_anything_is_decoded() {
    let layer = |p: &mut Parts| p.doc["documents"][0]["layers"][0].take();
    let refused = |bytes: Vec<u8>, what: &str| assert!(read(&bytes).is_err(), "{what}");
    refused(blank_set(MAX_IMAGES as u64 + 1, 1, (1, 1)), "65 images");
    refused(blank_set(1, MAX_LAYERS as u64 + 1, (1, 1)), "1001 layers");
    refused(blank_set(1, 1, (16384, 16384)), "1 GiB of pixels");
    assert!(read(&blank_set(MAX_IMAGES as u64, 2, (64, 64))).is_ok());
    refused(
        edited(|p| {
            p.doc["documents"][0]["layers"][1]["id"] = layer(p)["id"].clone();
            p.doc["documents"][0]["layers"][0] = layer(p)
        }),
        "a duplicate layer id",
    );
    refused(
        edited(|p| p.doc["documents"][0]["layers"][0]["blend"] = "sparkle".into()),
        "an unknown blend mode",
    );
    refused(
        edited(|p| p.doc["documents"][0]["layers"][0]["name"] = "a\u{202e}b".into()),
        "a bidi override",
    );
    refused(
        edited(|p| p.doc["documents"][0]["name"] = "a/b".into()),
        "a path in a file name",
    );
    refused(
        edited(|p| p.doc["documents"][0]["resolution"] = 1e9.into()),
        "a resolution out of range",
    );
    refused(
        edited(|p| {
            p.doc["documents"][0]["layers"][0]["text"] =
                json!({"text": "a", "settings": {"size": 1e6}})
        }),
        "a text size the Text tool can't make",
    );
    refused(
        edited(|p| {
            let (uid, lid) = order(&p.doc)[0];
            let tiles = p.tiles.remove(&(uid, lid)).unwrap();
            p.tiles.insert((uid, MAX_ID), tiles);
            p.doc["documents"][0]["layers"][0]["id"] = MAX_ID.into();
        }),
        "a layer id not exact in JavaScript",
    );
    refused(
        edited(|p| {
            let mut second = p.doc["documents"][0].clone();
            second["id"] = 1.into();
            p.doc["documents"][0]["id"] = 2.into();
            p.doc["documents"].as_array_mut().unwrap().push(second);
            for (_, lid) in order(&p.doc).into_iter().take(2) {
                let tiles = p.tiles[&(1, lid)].clone();
                p.tiles.insert((2, lid), tiles);
            }
        }),
        "images out of id order",
    );
}

#[test]
fn a_bad_tile_is_refused_by_the_merge_and_a_set_that_fails_to_load_changes_nothing() {
    let base = bytes(&app());
    // A one-colour tile cut short, and a PNG whose header claims another size.
    let bad_tile = |bytes: Rc<[u8]>| {
        edited(move |p| {
            let key = order(&p.doc)[1];
            let mut tiles = p.tiles[&key].to_vec();
            tiles[0] = bytes;
            p.tiles.insert(key, tiles.into());
        })
    };
    let png = encode(&(0u8..16).collect::<Vec<_>>(), 2, 2);
    for bad in [bad_tile(Rc::from(&[0u8, 1, 2, 3][..])), bad_tile(png)] {
        assert!(read(&bad).is_ok(), "it reads (tiles decode when used)");
        assert!(
            merge(&base, &base, &bad).is_err(),
            "the coordinator refuses it"
        );
        // Taken in whole, with a change of the image shown too: neither happens.
        let mut app = app();
        session(&mut app);
        let before = (
            app.documents.len(),
            pixel(&app, 1, 5, 5),
            app.active_ws().unwrap().document.id,
        );
        let mut parts = read(&bad).unwrap();
        let uid = documents(&read(&checkpoint(&app).unwrap()).unwrap().doc)
            .next()
            .unwrap()
            .0;
        let mut mine = read(&checkpoint(&app).unwrap()).unwrap();
        mine.doc["documents"][0]["layers"][1]["opacity"] = 7.into();
        parts.doc["documents"][0]["id"] = (uid + 1).into();
        let moved: Vec<_> = parts
            .tiles
            .drain()
            .map(|((_, lid), t)| ((uid + 1, lid), t))
            .collect();
        mine.tiles.extend(moved);
        let docs = mine.doc["documents"].as_array_mut().unwrap();
        docs.push(parts.doc["documents"][0].take());
        let done = queued(None, write(&mine).unwrap());
        step(&mut app, true, None);
        assert!(done.borrow().as_ref().is_some_and(|r| r.is_err()));
        let ws = app.active_ws().unwrap();
        assert_eq!(
            (app.documents.len(), pixel(&app, 1, 5, 5), ws.document.id),
            before
        );
        assert_eq!(
            ws.document.layers.get_at(1).unwrap().properties.opacity,
            255
        );
        disconnect();
    }
}

#[test]
fn undo_goes_to_the_session_only_between_gestures() {
    let mut app = app();
    session(&mut app);
    assert!(pdn_core::live::takes_undo(&app, false, false));
    // Mid-stroke, native Undo's own rules hold (it ignores Undo while a button is down).
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[
            Ev::Down(20.0, 20.0, MouseButton::Left),
            Ev::Move(40.0, 30.0),
        ],
    );
    assert!(!pdn_core::live::takes_undo(&app, false, false));
    drive_tool(
        &mut app,
        ToolType::PaintBrush,
        &[Ev::Up(40.0, 30.0, MouseButton::Left)],
    );
    assert!(pdn_core::live::takes_undo(&app, false, false));
    app.canvas_buttons = 1;
    assert!(
        !pdn_core::live::takes_undo(&app, false, false),
        "a held canvas button"
    );
    disconnect();
}

#[test]
fn a_change_still_waiting_fails_when_the_session_ends() {
    let mut app = app();
    let base = session(&mut app);
    let done = queued(Some(base.clone()), base);
    disconnect();
    assert_eq!(
        *done.borrow(),
        Some(Err("Live editing disconnected".into()))
    );
}

#[test]
fn the_tile_cache_keeps_only_the_layers_of_the_sets_images() {
    let mut app = app();
    session(&mut app);
    let keys = || {
        let mut k: Vec<_> = LIVE.with_borrow(|live| live.cache.keys().map(|k| k.1).collect());
        k.sort();
        k
    };
    let ids = |app: &AppWorkspace| {
        let mut k: Vec<_> = app
            .active_ws()
            .unwrap()
            .document
            .layers
            .iter()
            .map(|l| l.id)
            .collect();
        k.sort();
        k
    };
    pdn_core::action::perform(&mut app, "menu.layers.duplicate.layer");
    step(&mut app, true, None);
    assert_eq!(keys(), ids(&app));
    pdn_core::action::perform(&mut app, "menu.layers.delete.layer");
    step(&mut app, true, None);
    assert_eq!(keys(), ids(&app), "a deleted layer leaves nothing cached");
    disconnect();
}

#[test]
fn connecting_keeps_the_images_ids_a_checkpoint_before_it_gave() {
    let mut app = app();
    let pristine = checkpoint(&app).unwrap();
    connect();
    step(&mut app, true, None);
    assert_eq!(checkpoint(&app).unwrap(), pristine);
    disconnect();
}

/// `doc` and `sections` written as a checkpoint whatever their shapes, as anyone could send it.
fn raw(doc: &Value, sections: &[Tiles]) -> Vec<u8> {
    let json = pieces(doc);
    let mut out = MAGIC.to_vec();
    for (i, piece) in json.iter().enumerate() {
        put(&mut out, piece);
        if i + 1 == json.len() {
            out.extend(u32::MAX.to_le_bytes());
            break;
        }
        let tiles = sections.get(i).cloned().unwrap_or_else(|| Tiles::from([]));
        out.extend((tiles.len() as u32).to_le_bytes());
        for tile in tiles.iter() {
            put(&mut out, tile);
        }
    }
    out
}

#[test]
fn a_message_that_isnt_a_set_is_refused_without_a_panic() {
    let base = bytes(&app());
    for json in [
        r#"{"x":[]}"#,
        r#"{"id":1}"#,
        r#"{"documentz":[]}"#,
        "[]",
        "1",
        "null",
        r#"{"documents":{}}"#,
    ] {
        let message = raw(&serde_json::from_str(json).unwrap(), &[]);
        assert!(read(&message).is_err(), "{json}");
        assert!(
            merge(&base, &base, &message).is_err(),
            "{json} as a proposal"
        );
        assert!(merge(&message, &base, &base).is_err(), "{json} as the base");
    }
}

#[test]
fn an_image_the_others_would_refuse_stays_alone_and_never_poisons_the_set() {
    let mut app = app();
    let shared = app.active_ws().unwrap().id;
    let base = session(&mut app);
    // A resolution the others refuse (40,000 px/cm), and a text layer longer than they take.
    app.create_blank_document_in_new_workspace(32, 32, 96.0, false);
    let dpi = app.active_ws().unwrap().id;
    app.active_ws_mut().unwrap().document.resolution = 101_600.0;
    app.create_blank_document_in_new_workspace(32, 32, 96.0, false);
    let text = app.active_ws().unwrap().id;
    let long = TextSpec {
        text: "a".repeat(MAX_TEXT + 1),
        ..TextSpec::default()
    };
    app.active_ws_mut()
        .unwrap()
        .document
        .layers
        .get_at_mut(0)
        .unwrap()
        .text = Some(long);
    // A wider image than 65,536 pixels, which this build makes: the others take it in.
    app.create_blank_document_in_new_workspace(70_000, 2, 96.0, false);
    step(&mut app, true, None);
    let mine = checkpoint(&app).unwrap();
    assert!(read(&mine).is_ok(), "the set stays one everyone reads");
    let mut shared_widths = widths(&mine);
    shared_widths.sort();
    assert_eq!(shared_widths, [300, 70_000]);
    let why =
        |id: u64| LIVE.with_borrow(|live| live.alone.iter().find(|a| a.local == id).map(|a| a.why));
    assert_eq!(
        (why(dpi), why(text)),
        (Some(Why::Format), Some(Why::Format))
    );
    // Unsent paint on the shared image survives someone else's change.
    app.set_active_document_workspace(app.documents.iter().position(|w| w.id == shared));
    fill(&mut app, 1, (205, 105, 4, 4), [7, 7, 7, 255]);
    let uid = uid_here(&app, shared);
    let theirs = theirs_of(&base, |doc| {
        for d in doc["documents"].as_array_mut().unwrap() {
            if d["id"] == uid {
                d["layers"][0]["opacity"] = 9.into();
            }
        }
    });
    let done = queued(Some(base), theirs);
    step(&mut app, true, None);
    assert!(
        done.borrow().as_ref().is_some_and(|r| r.is_ok()),
        "{:?}",
        done.borrow()
    );
    assert_eq!(pixel(&app, 1, 205, 105), [7, 7, 7, 255]);
    disconnect();
}

#[test]
fn an_image_refused_for_its_bytes_is_tried_again_after_a_while_at_its_next_change() {
    MAX_BYTES_HERE.set(Some(48 << 10));
    let mut app = app();
    session(&mut app);
    // 160×160 of noise: about 100 KiB of tiles, over this test's 48 KiB.
    app.create_blank_document_in_new_workspace(160, 160, 96.0, false);
    let noisy = app.active_ws().unwrap().id;
    let noise = pdn_core::testing::fixture_rgba(160, 160, 9);
    let ws = app.active_ws_mut().unwrap();
    ws.document
        .layers
        .get_at_mut(0)
        .unwrap()
        .surface
        .data_mut()
        .copy_from_slice(&noise);
    step(&mut app, true, None);
    let alone = || {
        LIVE.with_borrow(|live| {
            live.alone
                .iter()
                .find(|a| a.local == noisy)
                .map(|a| (a.uid, a.why))
        })
    };
    let (uid, why) = alone().unwrap();
    assert_eq!(why, Why::Bytes);
    let cached = || LIVE.with_borrow(|live| live.cache.keys().filter(|k| k.0 == uid).count());
    assert_eq!(cached(), 0, "a refused try leaves nothing cached");
    let encodes = ENCODES.get();
    for i in 0..3 {
        fill(&mut app, 0, (i, i, 1, 1), [1, 2, 3, 255]);
        step(&mut app, true, None);
    }
    assert_eq!(ENCODES.get(), encodes, "not encoded again so soon");
    assert_eq!((alone(), cached()), (Some((uid, Why::Bytes)), 0));
    // A while after the try, the changes since are tried: still too large. The next wait is twice as long.
    let wait = || {
        LIVE.with_borrow(|live| {
            live.alone
                .iter()
                .find(|a| a.local == noisy)?
                .refused
                .map(|r| r.1)
        })
    };
    assert_eq!(wait(), Some(RETRY_MS));
    app.now_ms += RETRY_MS + 1000.0;
    step(&mut app, true, None);
    assert_eq!(ENCODES.get(), encodes + 1);
    assert_eq!(
        (alone(), cached(), wait()),
        (Some((uid, Why::Bytes)), 0, Some(2.0 * RETRY_MS))
    );
    step(&mut app, true, None);
    assert_eq!(ENCODES.get(), encodes + 1, "nothing new to try");
    // One colour over the noise: as many pixels, few bytes. Tried once the wait is over, it fits and joins under the
    // same id.
    fill(&mut app, 0, (0, 0, 160, 160), [1, 2, 3, 255]);
    app.now_ms += RETRY_MS + 1000.0;
    step(&mut app, true, None);
    assert_eq!(ENCODES.get(), encodes + 1);
    assert_eq!(alone(), Some((uid, Why::Bytes)));
    app.now_ms += RETRY_MS;
    step(&mut app, true, None);
    assert_eq!(ENCODES.get(), encodes + 2);
    assert_eq!(alone(), None);
    assert_eq!(uid_here(&app, noisy), uid);
    MAX_BYTES_HERE.set(None);
    disconnect();
}

#[test]
fn undoing_the_join_of_an_image_keeps_it_open_with_the_work_done_in_it() {
    let mut app = app();
    session(&mut app);
    let over = open_over_the_limit(&mut app);
    step(&mut app, true, None);
    // Work on it while it isn't shared, ending at the limit: it joins the set.
    let image = app.active_ws_mut().unwrap();
    image.document.remove_layer_at(MAX_LAYERS);
    fill(&mut app, 0, (1, 1, 1, 1), [5, 6, 7, 255]);
    step(&mut app, true, None);
    let with = checkpoint(&app).unwrap().to_vec();
    let uid = uid_here(&app, over);
    // This person's Undo takes it out of the set again.
    let without = theirs_of(&with, |doc| {
        doc["documents"]
            .as_array_mut()
            .unwrap()
            .retain(|d| d["id"] != uid)
    });
    let done = queued(Some(with), without);
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    let kept = ws(&app, over);
    assert_eq!(kept.document.layers.size(), MAX_LAYERS);
    assert_eq!(
        kept.document.layers.get_at(0).unwrap().surface.pixel(1, 1),
        [5, 6, 7, 255]
    );
    let why = LIVE.with_borrow(|live| live.alone.iter().find(|a| a.local == over).map(|a| a.why));
    assert_eq!(why, Some(Why::Removed));
    app.set_active_document_workspace(app.documents.iter().position(|w| w.id == over));
    assert!(
        !pdn_core::live::takes_undo(&app, false, false),
        "its Undo is native again"
    );
    // Changed, it rejoins.
    fill(&mut app, 0, (2, 2, 1, 1), [5, 6, 7, 255]);
    step(&mut app, true, None);
    assert_eq!(uid_here(&app, over), uid);
    disconnect();
}

#[test]
fn a_session_undo_shows_the_image_it_changed_only_when_that_change_comes_soon() {
    let mut app = app();
    session(&mut app);
    app.create_blank_document_in_new_workspace(16, 16, 96.0, false);
    step(&mut app, true, None);
    let second = uid_here(&app, app.documents[1].id);
    let change = |app: &AppWorkspace, opacity: u64| {
        let now = checkpoint(app).unwrap().to_vec();
        let edited = theirs_of(&now, |doc| {
            for d in doc["documents"].as_array_mut().unwrap() {
                if d["id"] == second {
                    d["layers"][0]["opacity"] = opacity.into();
                }
            }
        });
        queued(Some(now), edited)
    };
    app.set_active_document_workspace(Some(0));
    // An Undo that brought nothing (the shell had nothing of this person's to take back), then someone's change later.
    pdn_core::action::perform(&mut app, "menu.edit.undo");
    app.now_ms += FOCUS_MS + 1.0;
    change(&app, 9);
    step(&mut app, true, None);
    assert_eq!(app.active, Some(0), "no tab switch");
    // One whose change comes at once shows its image.
    pdn_core::action::perform(&mut app, "menu.edit.undo");
    change(&app, 8);
    step(&mut app, true, None);
    assert_eq!(app.active, Some(1));
    disconnect();
}

#[test]
fn undo_keeps_native_undo_through_a_move_float() {
    let mut app = app();
    session(&mut app);
    assert!(pdn_core::live::takes_undo(&app, false, false));
    use pdn_core::pixel::{IntRect, mask::Mask};
    use pdn_core::render::float::{Float, FloatSource, next_id};
    let source = pdn_core::document::surface::Surface::create(8, 8);
    let mask = Mask {
        rect: IntRect::new(0, 0, 8, 8),
        data: vec![255; 64],
    };
    let src = std::sync::Arc::new(FloatSource::new(&source, IntRect::new(0, 0, 8, 8), &mask));
    let float = Float {
        src,
        layer: 0,
        m: Default::default(),
        h: None,
        rev: next_id(),
    };
    app.active_ws_mut().unwrap().float = Some(std::sync::Arc::new(float));
    assert!(
        !pdn_core::live::takes_undo(&app, false, false),
        "a Move float"
    );
    app.active_ws_mut().unwrap().float = None;
    assert!(pdn_core::live::takes_undo(&app, false, false));
    disconnect();
}

/// A two-image set: a 70×70 image (a Background, a painted layer and a text layer) and a 20×20 one.
fn fuzz_base() -> Vec<u8> {
    let mut app = fresh_doc(70, 70, Some(4));
    let ws = app.active_ws_mut().unwrap();
    ws.is_initial_workspace = false;
    ws.add_layer(Layer::create_layer(70, 70, "Paint"));
    let mut text = Layer::create_layer(70, 70, "Text");
    text.text = Some(TextSpec {
        text: "Hi".into(),
        ..TextSpec::default()
    });
    ws.add_layer(text);
    fill(&mut app, 1, (10, 10, 30, 30), [200, 10, 10, 255]);
    app.create_blank_document_in_new_workspace(20, 20, 96.0, false);
    let mut cache = HashMap::new();
    let entries: Vec<_> = (app.documents.iter().zip([1, 2]))
        .map(|(ws, uid)| entry(&mut cache, ws, uid).unwrap())
        .collect();
    write(&set(entries.iter())).unwrap()
}

/// A step into a JSON value: an object's key or an array's index.
#[derive(Clone)]
enum Step {
    Key(String),
    Index(usize),
}

fn at<'a>(v: &'a mut Value, path: &[Step]) -> &'a mut Value {
    path.iter().fold(v, |v, step| match step {
        Step::Key(k) => &mut v[k.as_str()],
        Step::Index(i) => &mut v[*i],
    })
}

fn paths(v: &Value, path: &mut Vec<Step>, out: &mut Vec<Vec<Step>>) {
    out.push(path.clone());
    match v {
        Value::Object(m) => m.iter().for_each(|(k, v)| {
            path.push(Step::Key(k.clone()));
            paths(v, path, out);
            path.pop();
        }),
        Value::Array(items) => items.iter().enumerate().for_each(|(i, v)| {
            path.push(Step::Index(i));
            paths(v, path, out);
            path.pop();
        }),
        _ => {}
    }
}

/// What a peer could send from `base`: every `every`th byte set to each of a few values, cut short there; every JSON
/// value replaced by each of a few others; every key removed or renamed; every array item removed or doubled (the
/// JSON written back with the base's tile sections).
fn mutations(base: &[u8], every: usize) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for i in (MAGIC.len()..base.len()).step_by(every) {
        for v in [0u8, 1, 0x7f, 0xff] {
            let mut b = base.to_vec();
            b[i] = v;
            out.push(b);
        }
        out.push(base[..i].to_vec());
    }
    let parts = read(base).unwrap();
    let sections: Vec<Tiles> = order(&parts.doc)
        .iter()
        .map(|k| parts.tiles[k].clone())
        .collect();
    let values = [
        json!(null),
        json!(true),
        json!(-1),
        json!(1.5),
        json!(1e300),
        json!(u64::MAX),
        json!(1u64 << 52),
        json!("x"),
        json!(""),
        json!([]),
        json!({}),
        json!([{}]),
        json!({"id": 1}),
        json!({"x": []}),
    ];
    let mut all = Vec::new();
    paths(&parts.doc, &mut Vec::new(), &mut all);
    for path in &all {
        for v in &values {
            let mut doc = parts.doc.clone();
            *at(&mut doc, path) = v.clone();
            out.push(raw(&doc, &sections));
        }
        let mut doc = parts.doc.clone();
        match at(&mut doc, path) {
            Value::Object(m) => {
                for k in m.keys().cloned().collect::<Vec<_>>() {
                    let mut gone = Value::Object(m.clone());
                    gone.as_object_mut().unwrap().remove(&k);
                    let mut renamed = gone.clone();
                    renamed
                        .as_object_mut()
                        .unwrap()
                        .insert(format!("{k}x"), m[&k].clone());
                    for changed in [gone, renamed] {
                        let mut d = parts.doc.clone();
                        *at(&mut d, path) = changed;
                        out.push(raw(&d, &sections));
                    }
                }
            }
            Value::Array(items) => {
                for i in 0..items.len() {
                    let mut gone = items.clone();
                    gone.remove(i);
                    let mut doubled = items.clone();
                    doubled.insert(i, items[i].clone());
                    for changed in [gone, doubled] {
                        let mut d = parts.doc.clone();
                        *at(&mut d, path) = Value::Array(changed);
                        out.push(raw(&d, &sections));
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Puts each of `inputs` through `read`, `merge` in each place, and `apply` with and without a base on a connected app
/// that holds `base` (every `apply_every`th input): the panics.
fn panics(base: &[u8], inputs: &[Vec<u8>], apply_every: usize) -> Vec<String> {
    let mut app = fresh_doc(8, 8, None);
    connect();
    step(&mut app, true, None);
    let mut found = Vec::new();
    let mut go = |what: &str, f: &mut dyn FnMut()| {
        if let Err(e) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
            let e = e
                .downcast_ref::<String>()
                .cloned()
                .or(e.downcast_ref::<&str>().map(|s| s.to_string()));
            found.push(format!("{what}: {}", e.unwrap_or_default()));
        }
    };
    for (n, m) in inputs.iter().enumerate() {
        go("read", &mut || drop(read(m)));
        go("merge as the proposal", &mut || drop(merge(base, base, m)));
        go("merge as the current", &mut || drop(merge(base, m, base)));
        go("merge as the base", &mut || drop(merge(m, base, base)));
        if n % apply_every == 0 {
            go("apply", &mut || {
                queued(None, base.to_vec());
                step(&mut app, true, None);
                queued(Some(base.to_vec()), m.clone());
                step(&mut app, true, None);
                queued(None, m.clone());
                step(&mut app, true, None);
            });
        }
    }
    disconnect();
    found
}

#[test]
fn malformed_sets_never_panic() {
    let base = fuzz_base();
    let inputs = mutations(&base, 29);
    assert!(inputs.len() > 1000, "{}", inputs.len());
    let found = panics(&base, &inputs, 7);
    assert!(
        found.is_empty(),
        "{} panics: {:?}",
        found.len(),
        &found[..found.len().min(5)]
    );
}

/// The whole fuzz: every 3rd byte, and `apply` on every input (`cargo test -p paintrs --lib malformed -- --ignored`).
#[test]
#[ignore]
fn malformed_sets_never_panic_in_full() {
    let base = fuzz_base();
    let inputs = mutations(&base, 3);
    let found = panics(&base, &inputs, 1);
    eprintln!("{} inputs, {} panics", inputs.len(), found.len());
    assert!(found.is_empty(), "{:?}", &found[..found.len().min(10)]);
}

/// A PNG chunk, with its CRC.
fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut crc = !0u32;
    for &b in kind.iter().chain(body) {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                0xedb8_8320 ^ (crc >> 1)
            } else {
                crc >> 1
            };
        }
    }
    let mut out = (body.len() as u32).to_be_bytes().to_vec();
    out.extend(kind.iter().chain(body));
    out.extend((!crc).to_be_bytes());
    out
}

#[test]
fn a_tile_with_any_chunk_but_those_this_build_writes_is_refused_before_it_is_decoded() {
    let noise = pdn_core::testing::fixture_rgba(TILE, TILE, 3);
    let png = encode(&noise, TILE, TILE);
    assert_eq!(decode(&png, TILE, TILE).unwrap(), noise);
    // An ICC profile after the header: a small valid one here, though its zlib stream could inflate to gigabytes.
    let data = [0u8; 64];
    let (mut a, mut b) = (1u32, 0u32);
    for &x in &data {
        a = (a + u32::from(x)) % 65521;
        b = (b + a) % 65521;
    }
    let mut iccp = b"icc\0\0\x78\x01\x01\x40\x00\xbf\xff".to_vec();
    iccp.extend(data);
    iccp.extend(((b << 16) | a).to_be_bytes());
    let header = 8 + 25;
    let mut with = png[..header].to_vec();
    with.extend(chunk(b"iCCP", &iccp));
    with.extend(&png[header..]);
    let e = decode(&with, TILE, TILE).unwrap_err();
    assert!(e.contains("tile chunks"), "{e}");
    // Text after the image data, and bytes after the end, are refused too.
    let end = png.len() - 12;
    let mut text = png[..end].to_vec();
    text.extend(chunk(b"tEXt", b"a\0b"));
    text.extend(&png[end..]);
    assert!(decode(&text, TILE, TILE).is_err());
    let trailing = [&png[..], b"x"].concat();
    assert!(decode(&trailing, TILE, TILE).is_err());
    // The image data in a few IDATs decodes, in many it is refused: each costs a decoder pass.
    let idats = |n: usize| {
        let at = header + 8;
        let len = u32::from_be_bytes(png[header..header + 4].try_into().unwrap()) as usize;
        let data = &png[at..at + len];
        let mut out = png[..header].to_vec();
        for part in data.chunks(data.len().div_ceil(n)) {
            out.extend(chunk(b"IDAT", part));
        }
        out.extend(&png[at + len + 4..]);
        decode(&out, TILE, TILE)
    };
    assert_eq!(idats(4).unwrap(), noise);
    assert!(idats(5).unwrap_err().contains("tile chunks"));
}

#[test]
fn a_set_of_too_many_values_is_refused_before_it_is_parsed() {
    // 16 MiB of `0,`: a parse would hold 8 M values, hundreds of megabytes.
    let json = format!("[{}0]", "0,".repeat(8 << 20));
    let mut bytes = MAGIC.to_vec();
    put(&mut bytes, json.as_bytes());
    bytes.extend(u32::MAX.to_le_bytes());
    let e = read(&bytes).map(|_| ()).unwrap_err();
    assert!(e.contains("too many values"), "{e}");
    // Commas in a string aren't values.
    assert!(few_values(&[b"[\"", &[b','; MAX_VALUES + 1], b"\"]"]));
    assert!(!few_values(&[&[b','; MAX_VALUES + 1]]));
}

#[test]
fn a_set_of_too_many_objects_or_arrays_is_refused_before_it_is_parsed() {
    let refused = |json: String| {
        let mut bytes = MAGIC.to_vec();
        put(&mut bytes, json.as_bytes());
        bytes.extend(u32::MAX.to_le_bytes());
        let e = read(&bytes).map(|_| ()).unwrap_err();
        assert!(e.contains("too many values"), "{e}");
    };
    // Objects of one key, a few bytes each and hundreds parsed: flat, and nested 120 deep.
    let n = MAX_NESTS + 1;
    refused(format!("[{}0]", "{\"\":0},".repeat(n)));
    let deep = format!("{}0{}", "{\"\":".repeat(120), "}".repeat(120));
    refused(format!("[{}0]", format!("{deep},").repeat(n / 120 + 1)));
    let deep = format!("{}0{}", "[".repeat(120), "]".repeat(120));
    refused(format!("[{}0]", format!("{deep},").repeat(n / 120 + 1)));
    // A set at the limits stays within them: 64 images of 1,000 text layers.
    let mut app = fresh_doc(1, 1, None);
    let ws = app.active_ws_mut().unwrap();
    ws.document.layers.get_at_mut(0).unwrap().text = Some(TextSpec::default());
    for i in 1..MAX_LAYERS {
        let mut layer = Layer::create_layer(1, 1, &format!("Layer {i}"));
        layer.text = Some(TextSpec::default());
        ws.document.add_layer(layer);
    }
    let set = bytes(&app);
    let image = read(&set).unwrap();
    let json = pieces(&image.doc).concat();
    let count = |c: u8| json.iter().filter(|&&b| b == c).count();
    assert!(count(b'{').max(count(b'[')) * MAX_IMAGES < MAX_NESTS);
}

#[test]
fn undoing_a_rejoin_keeps_the_work_done_while_the_image_was_out_of_the_set() {
    let mut app = app();
    session(&mut app);
    let id = app.active_ws().unwrap().id;
    let uid = uid_here(&app, id);
    for i in 0..MAX_LAYERS {
        let layer = Layer::create_layer(300, 200, &format!("Layer {i}"));
        app.active_ws_mut().unwrap().document.add_layer(layer);
    }
    step(&mut app, true, None);
    let before = checkpoint(&app).unwrap().to_vec();
    // Work while it is out, with native Undo: back within the limits, it rejoins.
    while app.active_ws().unwrap().document.layers.size() > 2 {
        app.active_ws_mut().unwrap().document.remove_layer_at(2);
    }
    fill(&mut app, 1, (1, 1, 1, 1), [3, 3, 3, 255]);
    pdn_core::action::perform(&mut app, "menu.layers.duplicate.layer");
    step(&mut app, true, None);
    assert_eq!(uid_here(&app, id), uid);
    let rejoined = checkpoint(&app).unwrap().to_vec();
    let why = || LIVE.with_borrow(|live| live.alone.iter().find(|a| a.local == id).map(|a| a.why));
    let kept = |app: &AppWorkspace, x: u32| {
        let d = &ws(app, id).document;
        (
            d.layers.size(),
            d.layers.get_at(1).unwrap().surface.pixel(x, x),
        )
    };
    let layers_in_set = |app: &AppWorkspace| {
        let set = read(&checkpoint(app).unwrap()).unwrap();
        let (_, d) = documents(&set.doc).find(|(u, _)| *u == uid).unwrap();
        d["layers"].as_array().unwrap().len()
    };
    // This person's Undo of the rejoin: the others have the version from before it again, and the tab keeps the work
    // and its native Undo, alone.
    pdn_core::action::perform(&mut app, "menu.edit.undo");
    let done = queued(Some(rejoined.clone()), before.clone());
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    assert_eq!(kept(&app, 1), (3, [3, 3, 3, 255]));
    assert_eq!(layers_in_set(&app), 2);
    assert!(ws(&app, id).history.can_undo());
    assert_eq!(why(), Some(Why::Undone));
    assert!(!pdn_core::live::takes_undo(&app, false, false));
    // Their Redo of that (from another tab): the tab still waits for its next change, and then rejoins.
    redo_from_another_tab(&app);
    let done = queued(Some(before), rejoined.clone());
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    assert_eq!(
        (kept(&app, 1), why()),
        ((3, [3, 3, 3, 255]), Some(Why::Undone))
    );
    fill(&mut app, 1, (2, 2, 1, 1), [4, 4, 4, 255]);
    step(&mut app, true, None);
    assert_eq!((why(), uid_here(&app, id)), (None, uid));
    // A later change and the rejoin, undone one after the other: the tab keeps what it had at the rejoin.
    let again = checkpoint(&app).unwrap().to_vec();
    fill(&mut app, 1, (5, 5, 1, 1), [5, 5, 5, 255]);
    step(&mut app, true, None);
    let later = checkpoint(&app).unwrap().to_vec();
    pdn_core::action::perform(&mut app, "menu.edit.undo");
    let done = queued(Some(later), again.clone());
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    assert_eq!(
        kept(&app, 5),
        (3, [20, 40, 60, 255]),
        "the later change undone"
    );
    pdn_core::action::perform(&mut app, "menu.edit.undo");
    let done = queued(Some(again), rejoined);
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    assert_eq!(
        (kept(&app, 2), why()),
        ((3, [4, 4, 4, 255]), Some(Why::Undone))
    );
    disconnect();
}

#[test]
fn undoing_a_rejoin_after_someone_elses_change_keeps_the_work_and_their_change() {
    let mut app = app();
    session(&mut app);
    let id = app.active_ws().unwrap().id;
    for i in 0..MAX_LAYERS {
        let layer = Layer::create_layer(300, 200, &format!("Layer {i}"));
        app.active_ws_mut().unwrap().document.add_layer(layer);
    }
    step(&mut app, true, None);
    let before = checkpoint(&app).unwrap().to_vec();
    while app.active_ws().unwrap().document.layers.size() > 2 {
        app.active_ws_mut().unwrap().document.remove_layer_at(2);
    }
    fill(&mut app, 1, (1, 1, 1, 1), [3, 3, 3, 255]);
    pdn_core::action::perform(&mut app, "menu.layers.duplicate.layer");
    step(&mut app, true, None);
    let rejoined = checkpoint(&app).unwrap().to_vec();
    // Someone else changes the Background, which the rejoin didn't change.
    let theirs = theirs(&app, |doc| {
        doc["documents"][0]["layers"][0]["opacity"] = 9.into()
    });
    let done = queued(Some(rejoined.clone()), theirs);
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    // This person's Undo of the rejoin, over that change: the others keep it and get the version from before the
    // rejoin. The tab keeps both, alone.
    pdn_core::action::perform(&mut app, "menu.edit.undo");
    let done = queued(Some(rejoined), before);
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    let d = &ws(&app, id).document;
    let opacity = |d: &Document| d.layers.get_at(0).unwrap().properties.opacity;
    assert_eq!(
        (
            d.layers.size(),
            d.layers.get_at(1).unwrap().surface.pixel(1, 1),
            opacity(d)
        ),
        (3, [3, 3, 3, 255], 9)
    );
    let why = LIVE.with_borrow(|live| live.alone.iter().find(|a| a.local == id).map(|a| a.why));
    assert_eq!(why, Some(Why::Undone));
    let set = image(&checkpoint(&app).unwrap());
    assert_eq!((set.layers.size(), opacity(&set)), (2, 9));
    disconnect();
}

#[test]
fn redoing_an_undone_join_loads_the_image_into_its_tab_again() {
    let mut app = app();
    session(&mut app);
    app.create_blank_document_in_new_workspace(16, 16, 96.0, false);
    let id = app.active_ws().unwrap().id;
    step(&mut app, true, None);
    let with = checkpoint(&app).unwrap().to_vec();
    let uid = uid_here(&app, id);
    let without = theirs_of(&with, |doc| {
        doc["documents"]
            .as_array_mut()
            .unwrap()
            .retain(|d| d["id"] != uid)
    });
    let done = queued(Some(with.clone()), without.clone());
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    let tabs = app.documents.len();
    let done = queued(Some(without), with);
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    assert_eq!(
        (app.documents.len(), uid_here(&app, id)),
        (tabs, uid),
        "no second tab"
    );
    assert!(LIVE.with_borrow(|live| live.alone.is_empty()));
    disconnect();
}

#[test]
fn an_image_out_of_the_set_rejoins_once_this_persons_undo_took_its_version_out() {
    let mut app = app();
    session(&mut app);
    app.create_blank_document_in_new_workspace(4, 4, 96.0, false);
    let id = app.active_ws().unwrap().id;
    step(&mut app, true, None);
    let with = checkpoint(&app).unwrap().to_vec();
    let uid = uid_here(&app, id);
    for i in 0..MAX_LAYERS {
        let layer = Layer::create_layer(4, 4, &format!("Layer {i}"));
        app.active_ws_mut().unwrap().document.add_layer(layer);
    }
    step(&mut app, true, None);
    // This person's Undo of the join takes the set's version of it out.
    let without = theirs_of(&with, |doc| {
        doc["documents"]
            .as_array_mut()
            .unwrap()
            .retain(|d| d["id"] != uid)
    });
    let done = queued(Some(with), without);
    step(&mut app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    while app.active_ws().unwrap().document.layers.size() > 1 {
        app.active_ws_mut().unwrap().document.remove_layer_at(1);
    }
    fill(&mut app, 0, (1, 1, 1, 1), [2, 2, 2, 255]);
    step(&mut app, true, None);
    assert_eq!(uid_here(&app, id), uid);
    disconnect();
}

/// This person's Redo going to the session from another tab (the tab of an undone rejoin has native Undo and Redo).
fn redo_from_another_tab(app: &AppWorkspace) {
    LIVE.with_borrow_mut(|live| live.undoing = Some(app.now_ms));
}

/// The 300×200 image of [`app`] after it left the set over the layer limit and rejoined with `change`, made while it
/// was out.
fn rejoined(change: impl FnOnce(&mut DocumentWorkspace)) -> (AppWorkspace, u64) {
    let mut app = app();
    session(&mut app);
    let id = app.active_ws().unwrap().id;
    let ws = app.active_ws_mut().unwrap();
    for i in 0..MAX_LAYERS {
        let layer = Layer::create_layer(300, 200, &format!("Layer {i}"));
        ws.document.add_layer(layer);
    }
    step(&mut app, true, None);
    let ws = app.active_ws_mut().unwrap();
    while ws.document.layers.size() > 2 {
        ws.document.remove_layer_at(2);
    }
    change(ws);
    step(&mut app, true, None);
    assert!(LIVE.with_borrow(|live| live.alone.is_empty()), "rejoined");
    (app, id)
}

/// Someone else's change to the image's layers, with no Undo of this person's in flight: it loads into the tab, which
/// stays shared, and it stays in the set after this person's next stroke. The image as the set then has it.
fn theirs_loads(app: &mut AppWorkspace, id: u64, edit: impl FnOnce(&mut Vec<Value>)) -> Document {
    let before = checkpoint(app).unwrap().to_vec();
    let edited = theirs_of(&before, |doc| {
        edit(doc["documents"][0]["layers"].as_array_mut().unwrap())
    });
    let done = queued(Some(before), edited);
    step(app, true, None);
    assert_eq!(*done.borrow(), Some(Ok(None)));
    let why = LIVE.with_borrow(|live| live.alone.iter().find(|a| a.local == id).map(|a| a.why));
    assert_eq!(why, None, "still shared");
    fill(app, 1, (7, 7, 1, 1), [7, 7, 7, 255]);
    step(app, true, None);
    image(&checkpoint(app).unwrap())
}

fn opacity(d: &Document) -> u8 {
    d.layers.get_at(0).unwrap().properties.opacity
}

fn visible(d: &Document, i: usize) -> bool {
    d.layers.get_at(i).unwrap().properties.visible
}

#[test]
fn someone_elses_change_after_a_rejoin_and_its_revert_here_loads_as_theirs() {
    // The rejoin hid Layer 2, and this person shows it again. Someone else then changes the Background's opacity.
    let (mut app, id) =
        rejoined(|ws| ws.document.layers.get_at_mut(1).unwrap().properties.visible = false);
    app.active_ws_mut()
        .unwrap()
        .document
        .layers
        .get_at_mut(1)
        .unwrap()
        .properties
        .visible = true;
    step(&mut app, true, None);
    let set = theirs_loads(&mut app, id, |layers| layers[0]["opacity"] = 9.into());
    let d = &ws(&app, id).document;
    assert_eq!((opacity(d), visible(d, 1)), (9, true));
    assert_eq!((opacity(&set), visible(&set, 1)), (9, true));
    disconnect();
}

#[test]
fn someone_elses_change_that_reverts_a_rejoin_loads_as_theirs() {
    let (mut app, id) =
        rejoined(|ws| ws.document.layers.get_at_mut(1).unwrap().properties.visible = false);
    let set = theirs_loads(&mut app, id, |layers| {
        layers[1]["visible"] = true.into();
        layers[0]["opacity"] = 9.into();
    });
    let d = &ws(&app, id).document;
    assert_eq!((opacity(d), visible(d, 1)), (9, true));
    assert_eq!((opacity(&set), visible(&set, 1)), (9, true));
    disconnect();
}

#[test]
fn someone_else_deleting_the_layer_a_rejoin_added_loads_as_theirs() {
    let (mut app, id) = rejoined(|ws| {
        let copy = Layer::create_layer(300, 200, "Copy");
        ws.document.add_layer(copy);
    });
    let set = theirs_loads(&mut app, id, |layers| {
        layers.remove(2);
        layers[0]["opacity"] = 9.into();
    });
    let d = &ws(&app, id).document;
    assert_eq!((d.layers.size(), opacity(d)), (2, 9));
    assert_eq!((set.layers.size(), opacity(&set)), (2, 9));
    disconnect();
}
