use ratatui::layout::Rect;

use crate::model::{DetailSpan, FieldId};

#[derive(Clone, Debug, Default)]
pub struct Screen {
    pub hits: Hits,
    pub continues: Vec<Option<u16>>,
    pub value_width: usize,
    pub detail_height: usize,
    pub detail_lines: usize,
    pub detail_scroll: usize,
    pub list_height: usize,
    pub list_offset: usize,
    pub spans: Vec<DetailSpan>,
}

#[derive(Clone, Debug, Default)]
pub struct Hits {
    pub new_repo: Option<Rect>,
    pub refresh: Option<Rect>,
    pub copy_clone: Option<Rect>,
    pub copy_web: Option<Rect>,
    pub filter: Option<Rect>,
    pub list_area: Option<Rect>,
    pub detail_area: Option<Rect>,
    pub list_rows: Vec<(Rect, usize)>,
    pub branch_rows: Vec<(Rect, usize)>,
    pub commit_rows: Vec<(Rect, usize)>,
    pub pull_rows: Vec<(Rect, usize)>,
    pub fields: Vec<FieldHit>,
    pub modal: Option<Rect>,
    pub modal_inputs: Vec<Rect>,
}

#[derive(Clone, Copy, Debug)]
pub struct FieldHit {
    pub id: FieldId,
    pub rect: Rect,
}

pub fn contains(rect: Rect, x: u16, y: u16) -> bool {
    x >= rect.x && y >= rect.y && x < rect.right() && y < rect.bottom()
}

pub fn hit(rects: &[(Rect, usize)], x: u16, y: u16) -> Option<usize> {
    rects
        .iter()
        .rev()
        .find(|(rect, _)| contains(*rect, x, y))
        .map(|(_, index)| *index)
}
