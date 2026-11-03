//! Which apps the dock shows, in what order, and where a dragged one ends up: Finder first,
//! then the pinned apps, then the running apps that aren't pinned, as the native Dock has them.

/// Finder's key. It always comes first, and can't be moved or unpinned.
pub const FINDER: &str = "id:com.apple.finder";

/// An app's identity across launches: its bundle identifier, else its bundle's path, else (for
/// a process with no bundle) its pid.
pub fn key(bundle_id: Option<&str>, path: Option<&str>, pid: Option<i32>) -> String {
    match (bundle_id, path, pid) {
        (Some(id), _, _) => format!("id:{id}"),
        (None, Some(path), _) => format!("path:{path}"),
        (None, None, pid) => format!("pid:{}", pid.unwrap_or(0)),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub key: String,
    pub pid: i32,
    pub path: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pinned {
    pub key: String,
    pub path: String,
}

/// One app in the row.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub key: String,
    /// Its process, while it runs.
    pub pid: Option<i32>,
    pub path: Option<String>,
    pub pinned: bool,
}

/// The row: Finder, the pinned apps (running or not), then the other running apps in
/// `running`'s order. Without `pinning`, just the running apps.
pub fn desired(pins: &[Pinned], running: &[Running], pinning: bool) -> Vec<Spec> {
    let mut row: Vec<Spec> = Vec::new();
    let spec =
        |app: &Running, pinned| Spec { key: app.key.clone(), pid: Some(app.pid), path: app.path.clone(), pinned };
    if let Some(finder) = running.iter().find(|app| app.key == FINDER) {
        row.push(spec(finder, false));
    }
    if pinning {
        for pin in pins {
            if pin.key == FINDER || row.iter().any(|spec| spec.key == pin.key) {
                continue;
            }
            let pid = running.iter().find(|app| app.key == pin.key).map(|app| app.pid);
            row.push(Spec { key: pin.key.clone(), pid, path: Some(pin.path.clone()), pinned: true });
        }
    }
    for app in running {
        if !row.iter().any(|spec| spec.key == app.key) {
            row.push(spec(app, false));
        }
    }
    row
}

/// What dropping an app somewhere in the row changes.
#[derive(Clone, Debug, PartialEq)]
pub enum Drop {
    /// The pinned apps, in their new order, by key.
    Pins(Vec<String>),
    /// The unpinned running apps, in their new order, by key.
    Running(Vec<String>),
}

/// `dragged` dropped at `index` in `row`, which is the row without it. A pinned app stays
/// pinned, moving among the pinned ones; an unpinned one dropped among (or just after) the
/// pinned ones is pinned there, and one dropped further along moves among the running apps.
pub fn drop(row: &[Spec], dragged: &Spec, index: usize, pinning: bool) -> Drop {
    let start = usize::from(row.first().is_some_and(|spec| spec.key == FINDER));
    let index = index.clamp(start, row.len());
    let pinned: Vec<String> = row.iter().filter(|spec| spec.pinned).map(|spec| spec.key.clone()).collect();
    let pinned_end = start + pinned.len();
    if pinning && dragged.path.is_some() && (dragged.pinned || index <= pinned_end) {
        let mut pins = pinned;
        pins.insert(index.min(pinned_end) - start, dragged.key.clone());
        Drop::Pins(pins)
    } else {
        let mut running: Vec<String> =
            row[start..].iter().filter(|spec| !spec.pinned).map(|spec| spec.key.clone()).collect();
        running.insert(index.saturating_sub(pinned_end).min(running.len()), dragged.key.clone());
        Drop::Running(running)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn running(names: &[&str]) -> Vec<Running> {
        names
            .iter()
            .enumerate()
            .map(|(i, name)| Running { key: name.to_string(), pid: i as i32 + 1, path: Some(format!("/{name}")) })
            .collect()
    }

    fn pins(names: &[&str]) -> Vec<Pinned> {
        names.iter().map(|name| Pinned { key: name.to_string(), path: format!("/{name}") }).collect()
    }

    fn keys(row: &[Spec]) -> Vec<&str> {
        row.iter().map(|spec| spec.key.as_str()).collect()
    }

    #[test]
    fn finder_then_pinned_then_running() {
        let row = desired(&pins(&["mail", "notes"]), &running(&["kitty", FINDER, "notes"]), true);
        assert_eq!(keys(&row), [FINDER, "mail", "notes", "kitty"]);
        assert_eq!(row[1].pid, None);
        assert_eq!(row[2].pid, Some(3));
        assert!(row[2].pinned && !row[3].pinned);
    }

    #[test]
    fn without_pinning_only_running_apps_show() {
        let row = desired(&pins(&["mail"]), &running(&["kitty", FINDER]), false);
        assert_eq!(keys(&row), [FINDER, "kitty"]);
    }

    #[test]
    fn a_pinned_app_listed_twice_shows_once() {
        let row = desired(&pins(&["mail", "mail", FINDER]), &running(&[FINDER]), true);
        assert_eq!(keys(&row), [FINDER, "mail"]);
    }

    #[test]
    fn keys_prefer_the_bundle_identifier() {
        assert_eq!(key(Some("a.b"), Some("/x.app"), Some(4)), "id:a.b");
        assert_eq!(key(None, Some("/x.app"), Some(4)), "path:/x.app");
        assert_eq!(key(None, None, Some(4)), "pid:4");
    }

    fn row_without(row: &[Spec], key: &str) -> (Vec<Spec>, Spec) {
        let dragged = row.iter().find(|spec| spec.key == key).unwrap().clone();
        (row.iter().filter(|spec| spec.key != key).cloned().collect(), dragged)
    }

    #[test]
    fn moving_a_pinned_app_reorders_the_pins() {
        let row = desired(&pins(&["a", "b", "c"]), &running(&[FINDER, "x"]), true);
        let (rest, dragged) = row_without(&row, "a");
        // Finder, b, c, x: dropped between c and x, it stays pinned, at the end of the pins.
        assert_eq!(drop(&rest, &dragged, 3, true), Drop::Pins(vec!["b".into(), "c".into(), "a".into()]));
        // Dropped among the running apps, it still stays with the pins.
        assert_eq!(drop(&rest, &dragged, 4, true), Drop::Pins(vec!["b".into(), "c".into(), "a".into()]));
        // Nothing goes before Finder.
        assert_eq!(drop(&rest, &dragged, 0, true), Drop::Pins(vec!["a".into(), "b".into(), "c".into()]));
    }

    #[test]
    fn dropping_a_running_app_among_the_pins_pins_it() {
        let row = desired(&pins(&["a"]), &running(&[FINDER, "x", "y"]), true);
        let (rest, dragged) = row_without(&row, "y");
        assert_eq!(drop(&rest, &dragged, 1, true), Drop::Pins(vec!["y".into(), "a".into()]));
        // Just after the last pin counts as among them.
        assert_eq!(drop(&rest, &dragged, 2, true), Drop::Pins(vec!["a".into(), "y".into()]));
    }

    #[test]
    fn dropping_a_running_app_further_along_reorders_the_running_apps() {
        let row = desired(&pins(&["a"]), &running(&[FINDER, "x", "y", "z"]), true);
        let (rest, dragged) = row_without(&row, "z");
        // Finder, a, x, y: between x and y.
        assert_eq!(drop(&rest, &dragged, 3, true), Drop::Running(vec!["x".into(), "z".into(), "y".into()]));
    }

    #[test]
    fn without_pinning_every_drop_reorders_the_running_apps() {
        let row = desired(&[], &running(&[FINDER, "x", "y"]), false);
        let (rest, dragged) = row_without(&row, "y");
        assert_eq!(drop(&rest, &dragged, 1, false), Drop::Running(vec!["y".into(), "x".into()]));
    }

    #[test]
    fn an_app_without_a_bundle_cannot_be_pinned() {
        let mut apps = running(&[FINDER, "x", "tool"]);
        apps[2].path = None;
        let row = desired(&pins(&["a"]), &apps, true);
        let (rest, dragged) = row_without(&row, "tool");
        assert_eq!(drop(&rest, &dragged, 1, true), Drop::Running(vec!["tool".into(), "x".into()]));
    }
}
