use super::*;

pub const MAX_OPEN: usize = 4;

#[derive(Default, Serialize, Deserialize)]
pub(super) struct OpenProject {
    pub open: Vec<String>,
    pub writer: Option<String>,
}

pub(super) fn current_lock(home: &Path) -> io::Result<fs::File> {
    fs::create_dir_all(leads_dir(home))?;
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(leads_dir(home).join("current.lock"))?;
    lock.lock()?;
    Ok(lock)
}

pub(super) fn read_open(home: &Path, current: &BTreeMap<String, String>) -> io::Result<BTreeMap<String, OpenProject>> {
    let mut projects: BTreeMap<String, OpenProject> = match fs::File::open(leads_dir(home).join("open.json")) {
        Ok(file) => {
            let mut bytes = Vec::new();
            file.take(1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 1024 * 1024 { return Err(io::Error::other("열린 총괄 목록이 너무 큽니다")); }
            serde_json::from_slice(&bytes).map_err(io::Error::other)?
        }
        Err(error) if error.kind() == ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(error),
    };
    for (project, id) in current.iter().filter(|(_, id)| is_run_id(id)) {
        projects.entry(project.clone()).or_insert_with(|| OpenProject { open: vec![id.clone()], writer: Some(id.clone()) });
    }
    for (project, tabs) in &mut projects {
        if overall::is_overall(home, project) { tabs.writer = None; }
    }
    Ok(projects)
}

fn write_open(home: &Path, projects: &BTreeMap<String, OpenProject>) -> io::Result<()> {
    let path = leads_dir(home).join("open.json");
    let temp = path.with_extension(format!("{}.tmp", new_run_id(Utc::now())));
    fs::write(&temp, store::to_pretty(projects)?)?;
    fs::rename(temp, path)
}

pub fn open_threads(home: &Path, project: &str) -> Vec<String> {
    read_current(home).and_then(|current| read_open(home, &current)).ok()
        .and_then(|mut projects| projects.remove(&project_key(project)))
        .map(|tabs| tabs.open.into_iter().filter(|id| is_run_id(id)).collect()).unwrap_or_default()
}

pub fn is_open(home: &Path, project: &str, thread: &str) -> bool {
    is_run_id(thread) && open_threads(home, project).iter().any(|id| id == thread)
}

pub(super) fn is_writer(home: &Path, project: &str, thread: &str) -> Result<bool, String> {
    let current = read_current(home).map_err(|e| e.to_string())?;
    let projects = read_open(home, &current).map_err(|e| e.to_string())?;
    Ok(projects.get(&project_key(project)).is_some_and(|tabs| tabs.writer.as_deref() == Some(thread)))
}

/// Attach to an open tab without selecting it; create the first conversation if needed.
pub fn attachment_thread(home: &Path, project: &str, thread: Option<&str>) -> Result<String, String> {
    let Some(id) = thread else { return select_thread(home, project, None, false) };
    let _lock = current_lock(home).map_err(|e| e.to_string())?;
    if !is_open(home, project, id)
        || thread_project(&read_thread(home, id)).is_none_or(|owner| project_key(&owner) != project_key(project)) {
        return Err("이 프로젝트의 열린 총괄 대화가 아닙니다".into());
    }
    Ok(id.to_string())
}

/// Resolve an addressed request under the same lock as closing and changing writer.
pub fn select_thread(home: &Path, project: &str, thread: Option<&str>, fresh: bool) -> Result<String, String> {
    if thread.is_some() && fresh { return Err("대화 지정과 새 대화를 함께 요청할 수 없습니다".into()); }
    let _lock = current_lock(home).map_err(|e| e.to_string())?;
    let mut current = read_current(home).map_err(|e| e.to_string())?;
    let mut projects = read_open(home, &current).map_err(|e| e.to_string())?;
    let key = project_key(project);
    let tabs = projects.entry(key.clone()).or_default();
    if let Some(id) = thread {
        if !is_run_id(id) || !tabs.open.iter().any(|open| open == id)
            || thread_project(&read_thread(home, id)).is_none_or(|owner| project_key(&owner) != key) {
            return Err("이 프로젝트의 열린 총괄 대화가 아닙니다".into());
        }
        if current.get(&key).map(String::as_str) != Some(id) {
            current.insert(key, id.to_string());
            write_current(home, &current).map_err(|e| e.to_string())?;
        }
        return Ok(id.to_string());
    }
    if let Some(id) = current.get(&key).filter(|id| !fresh && is_run_id(id) && tabs.open.contains(id)) {
        return Ok(id.clone());
    }
    if tabs.open.len() >= MAX_OPEN { return Err("열린 총괄은 프로젝트마다 최대 4개입니다. 탭을 닫고 다시 여세요".into()); }
    let id = new_run_id(Utc::now());
    fs::create_dir_all(thread_dir(home, &id)).map_err(|e| e.to_string())?;
    append(home, &id, &LeadEvent::Start { at: now_iso(), thread_id: id.clone(), project: project.to_string() }).map_err(|e| e.to_string())?;
    fs::write(sessions_path(home, &id), "").map_err(|e| e.to_string())?;
    if tabs.open.is_empty() && !overall::is_overall(home, project) { tabs.writer = Some(id.clone()); }
    tabs.open.push(id.clone());
    write_open(home, &projects).map_err(|e| e.to_string())?;
    current.insert(key, id.clone());
    write_current(home, &current).map_err(|e| e.to_string())?;
    Ok(id)
}

fn idle_lock(home: &Path, thread: &str) -> Result<fs::File, String> {
    let lock = turn_lock(home, thread).map_err(|e| e.to_string())?;
    // `show` briefly holds this lock while checking the turn.
    let deadline = Instant::now() + Duration::from_millis(300);
    loop {
        match lock.try_lock() {
            Ok(()) => break,
            Err(fs::TryLockError::WouldBlock) if running_path(home, thread).exists() => return Err(BUSY.into()),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Err(fs::TryLockError::WouldBlock) => return Err(BUSY.into()),
            Err(fs::TryLockError::Error(error)) => return Err(error.to_string()),
        }
    }
    if running_path(home, thread).exists() { return Err(UNKNOWN.into()); }
    Ok(lock)
}

pub fn close(home: &Path, thread: &str) -> Result<(), String> {
    if !is_run_id(thread) { return Err("총괄 대화 ID가 올바르지 않습니다".into()); }
    let _lock = current_lock(home).map_err(|e| e.to_string())?;
    let mut current = read_current(home).map_err(|e| e.to_string())?;
    let mut projects = read_open(home, &current).map_err(|e| e.to_string())?;
    let project = thread_project(&read_thread(home, thread)).ok_or("총괄 대화가 없습니다")?;
    let key = project_key(&project);
    let tabs = projects.get_mut(&key).filter(|tabs| tabs.open.iter().any(|id| id == thread)).ok_or("열린 총괄 대화가 아닙니다")?;
    let _turn = idle_lock(home, thread)?;
    tabs.open.retain(|id| id != thread);
    if tabs.writer.as_deref() == Some(thread) { tabs.writer = tabs.open.first().cloned(); }
    if tabs.open.is_empty() {
        current.remove(&key);
        projects.remove(&key);
    } else if current.get(&key).map(String::as_str) == Some(thread) {
        current.insert(key, tabs.open.last().unwrap().clone());
    }
    // Remove the legacy fallback first, so a closed last tab cannot be resurrected.
    write_current(home, &current).map_err(|e| e.to_string())?;
    write_open(home, &projects).map_err(|e| e.to_string())
}

pub fn set_writer(home: &Path, thread: &str) -> Result<(), String> {
    if !is_run_id(thread) { return Err("총괄 대화 ID가 올바르지 않습니다".into()); }
    let _lock = current_lock(home).map_err(|e| e.to_string())?;
    let current = read_current(home).map_err(|e| e.to_string())?;
    let mut projects = read_open(home, &current).map_err(|e| e.to_string())?;
    let project = thread_project(&read_thread(home, thread)).ok_or("총괄 대화가 없습니다")?;
    let tabs = projects.get_mut(&project_key(&project)).filter(|tabs| tabs.open.iter().any(|id| id == thread)).ok_or("열린 총괄 대화가 아닙니다")?;
    if overall::is_overall(home, &project) { return Err("전체 총괄은 직접 수정을 맡을 수 없습니다".into()); }
    let _target = idle_lock(home, thread)?;
    let _source = tabs.writer.as_deref().filter(|id| *id != thread).map(|id| idle_lock(home, id)).transpose()?;
    tabs.writer = Some(thread.to_string());
    write_open(home, &projects).map_err(|e| e.to_string())
}

/// Keep a proven dispatch/followup origin even after close; standalone runs have none.
pub fn run_thread(home: &Path, project: &str, run_id: &str) -> Option<String> {
    if !is_run_id(run_id) { return None; }
    thread_ids(home).into_iter().find(|id| {
        let events = read_thread(home, id);
        thread_project(&events).is_some_and(|owner| project_key(&owner) == project_key(project))
            && events.iter().any(|event| matches!(event, LeadEvent::Dispatch { run_id: id, .. } | LeadEvent::Followup { run_id: id, .. } if id == run_id))
    })
}
