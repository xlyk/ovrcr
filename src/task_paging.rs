//! Byte-bounded list pages for the task control protocol.
use crate::protocol::MAX_FRAME_BYTES;
use crate::task_manager::TaskResponse;
use crate::tasks::{Run, Task, TaskId};
use anyhow::{Context, Result, bail};
use serde::Serialize;

// Reserve transport headroom plus the response discriminant, vector length and
// optional cursor. Bincode's maximum encoding of those fields fits in 32 bytes.
const ITEM_BUDGET: usize = MAX_FRAME_BYTES - 1024 - 32;
fn page<'a, T: Serialize + Clone + 'a>(
    rows: impl Iterator<Item = (u64, &'a T)>,
) -> Result<(Vec<T>, Option<usize>)> {
    let mut items = Vec::new();
    let mut bytes = 0;
    let mut last_id = 0;
    for (id, row) in rows {
        let size = bincode::serde::encode_to_vec(row, bincode::config::standard())
            .context("serialize list row")?
            .len();
        if size > ITEM_BUDGET - bytes {
            if items.is_empty() {
                bail!("task or run record is too large for a list page");
            }
            let next = usize::try_from(last_id).context("list page cursor overflow")?;
            return Ok((items, Some(next)));
        }
        bytes += size;
        items.push(row.clone());
        last_id = id;
    }
    Ok((items, None))
}
// The wire field is named offset for compatibility; it carries the last row ID,
// with zero starting a traversal. Changes before that ID cannot shift a page.
pub fn tasks(rows: &[Task], offset: usize) -> Result<TaskResponse> {
    let mut rows = rows
        .iter()
        .filter(|task| task.id.0 > offset as u64)
        .collect::<Vec<_>>();
    rows.sort_unstable_by_key(|task| task.id);
    let (items, next_offset) = page(rows.into_iter().map(|task| (task.id.0, task)))?;
    Ok(TaskResponse::TasksPage { items, next_offset })
}
pub fn runs(rows: &[Run], task: Option<TaskId>, offset: usize) -> Result<TaskResponse> {
    let mut rows = rows
        .iter()
        .filter(|run| task.is_none_or(|id| run.task_id == id))
        .filter(|run| offset == 0 || run.id.0 < offset as u64)
        .collect::<Vec<_>>();
    rows.sort_unstable_by_key(|run| std::cmp::Reverse(run.id));
    let (items, next_offset) = page(rows.into_iter().map(|run| (run.id.0, run)))?;
    Ok(TaskResponse::RunsPage { items, next_offset })
}
