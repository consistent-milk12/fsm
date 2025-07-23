UIOverlay::ContentSearch => match key_event.code {
    KeyCode::Char(c) => {
        let mut app = self.app.lock().await;
        app.ui.input.push(c);

        // Reset results *and* selection
        app.search_results.clear();
        app.rich_search_results.clear();
        app.raw_search_results = None;
        app.ui.last_query     = None;
        app.ui.selected       = None;          // <- NEW
        app.redraw            = true;          // <- NEW
        Action::NoOp
    }

    KeyCode::Backspace => {
        let mut app = self.app.lock().await;
        app.ui.input.pop();

        // Same reset logic
        app.search_results.clear();
        app.rich_search_results.clear();
        app.raw_search_results = None;
        app.ui.last_query     = None;
        app.ui.selected       = None;          // <- NEW
        app.redraw            = true;          // <- NEW
        Action::NoOp
    }

    KeyCode::Enter => {
        let app = self.app.lock().await;

        // ---------- RAW SEARCH RESULTS ----------
        if let (Some(raw), Some(idx)) = (&app.raw_search_results, app.ui.selected) {
            if let Some(line) = raw.lines.get(idx) {
                if let Some((p, n)) =
                     RawSearchResult::parse_file_info_with_base(line, &raw.base_directory)
                {
                    return Action::OpenFile(p, n);
                }
            }
            // We had raw results but couldn't parse → nothing to do
            return Action::NoOp;
        }

        // ---------- RICH SEARCH RESULTS ----------
        if let (false, Some(idx)) = (!app.rich_search_results.is_empty(), app.ui.selected) {
            if let Some(line) = app.rich_search_results.get(idx) {
                let base = app.fs.active_pane().cwd.clone();
                if let Some((p, n)) = RawSearchResult::parse_file_info_with_base(line, &base) {
                    return Action::OpenFile(p, n);
                }
            }
        }

        // ---------- SIMPLE SEARCH RESULTS ----------
        if let (false, Some(idx)) = (!app.search_results.is_empty(), app.ui.selected) {
            if let Some(res) = app.search_results.get(idx) {
                return Action::OpenFile(res.path.clone(), None);
            }
        }

        // No selection → launch a new search
        Action::ContentSearch(app.ui.input.clone())
    }

    KeyCode::Up => {
        let mut app = self.app.lock().await;
        let result_count = current_result_count(&app);
        if result_count > 0 {
            let new_idx = app.ui.selected.unwrap_or(0).saturating_sub(1);
            app.ui.selected = Some(new_idx);
            app.redraw = true;                 // <- NEW
        }
        Action::NoOp
    }

    KeyCode::Down => {
        let mut app = self.app.lock().await;
        let result_count = current_result_count(&app);
        if result_count > 0 {
            let cur  = app.ui.selected.unwrap_or(0);
            let new_idx = (cur + 1).min(result_count - 1);
            app.ui.selected = Some(new_idx);
            app.redraw = true;                 // <- NEW
        }
        Action::NoOp
    }

    _ => Action::NoOp,
}
