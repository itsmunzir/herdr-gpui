use super::*;

/// The middle of cell `col` of row `row`.
fn cell(
    view: &Entity<HerdrWindow>,
    cx: &mut VisualTestContext,
    col: u16,
    row: u16,
) -> Point<Pixels> {
    view.read_with(cx, |view, _| {
        let height = view.config.terminal.line_height();
        view.bounds.origin
            + point(
                px((f32::from(col) + 0.5) * view.cell_width),
                px((f32::from(row) + 0.5) * height),
            )
    })
}

#[gpui::test]
fn a_wrapped_path_is_underlined_on_every_row_and_opens_from_any(cx: &mut gpui::TestAppContext) {
    let root = tempfile::tempdir().unwrap();
    let folder = "some/deeply/nested/folder/with/many/levels";
    std::fs::create_dir_all(root.path().join(folder)).unwrap();
    std::fs::write(root.path().join(folder).join("notes.txt"), "notes").unwrap();
    // In the fixture's 60 columns the path starts at column 15 and wraps
    // after `levels/no`, going on for seven columns of the second row.
    let (view, cx) = window(
        &format!("the file is at {folder}/notes.txt now"),
        root.path(),
        cx,
    );
    let head = cell(&view, cx, 20, 0);
    let tail = cell(&view, cx, 3, 1);

    // Held over the second row, the link modifier has the whole path looked
    // for on disk; found, every row of it is underlined.
    cx.simulate_mouse_move(tail, None, Modifiers::secondary_key());
    cx.run_until_parked();
    let rows = vec![(0, 15..60), (1, 0..7)];
    view.read_with(cx, |view, _| {
        let link = view.hovered_local_link().unwrap();
        assert_eq!(link.rows().collect::<Vec<_>>(), rows);
        for position in [head, tail] {
            assert!(view.terminal_link_hovered(position, Modifiers::secondary_key()));
        }
    });
    // Moving onto the first row keeps the same link.
    cx.simulate_mouse_move(head, None, Modifiers::secondary_key());
    cx.run_until_parked();
    view.read_with(cx, |view, _| {
        let link = view.hovered_local_link().unwrap();
        assert_eq!(link.rows().collect::<Vec<_>>(), rows);
    });

    // A click on the second row's part opens the whole path.
    let alt = Modifiers {
        alt: true,
        ..Modifiers::secondary_key()
    };
    modifier_click(cx, tail, alt);
    let real = real_path(&root.path().join(folder).join("notes.txt")).unwrap();
    let expected = url::Url::from_file_path(real).unwrap();
    assert_eq!(cx.opened_url().as_deref(), Some(expected.as_str()));
}
