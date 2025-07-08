use std::{borrow::Cow, path::PathBuf};

use rayon::{
    iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator},
    slice::ParallelSliceMut,
};

/// Applies greedy matching logic to filter and sort pre-processed data.
/// Expect a slice of tuples, the first pos representing the element to
/// which  greedy matching logic is applied to, and the second
/// the element which will be presented.
pub fn greedy_match_filter<'a>(
    target: Vec<u8>,
    pre_processed: &'a [(Vec<u8>, Cow<'a, str>)],
) -> Vec<Cow<'a, str>> {
    let mut matched: Vec<(i32, Cow<str>)> = pre_processed
        .par_iter()
        .filter_map(|(value, disp)| {
            let (is_match, score) = greedy_match_score(&target, value);
            if is_match {
                Some((score, disp.clone()))
            } else {
                None
            }
        })
        .collect();

    matched.par_sort_unstable_by(|a, b| b.0.cmp(&a.0));

    matched
        .into_par_iter()
        .map(|(_, path)| path)
        .collect::<Vec<Cow<str>>>()
}

/// Sub-sequence scoring logic.
fn greedy_match_score(query_bytes: &[u8], target_bytes: &[u8]) -> (bool, i32) {
    let mut q_idx = 0;
    let mut score = 0;
    let mut last_match_idx = None;
    let mut first_match_idx = None;

    for (t_idx, &t_char) in target_bytes.iter().enumerate() {
        if q_idx == query_bytes.len() {
            break;
        }

        if t_char == query_bytes[q_idx] {
            score += 10;

            if let Some(last) = last_match_idx {
                let gap = t_idx - last;
                if gap <= 1 {
                    score += 5;
                }
            } else {
                first_match_idx = Some(t_idx);
            }

            last_match_idx = Some(t_idx);
            q_idx += 1;
        }
    }

    let is_match = q_idx == query_bytes.len();
    // TODO potentially allow 1 deviation?
    //let is_match = q_idx + 1 >= query_bytes.len();

    if is_match {
        if let Some(first_idx) = first_match_idx {
            score += 20 - first_idx.min(20);
        }
        (true, score as i32)
    } else {
        (false, 0)
    }
}

pub fn prepare_fuzzy_target(target: &str) -> Vec<u8> {
    target
        .to_lowercase()
        .bytes()
        .filter(|b| *b != b' ')
        .collect()
}

pub fn prepare_paths_for_seach<'a>(paths: &'a [PathBuf]) -> Vec<(Vec<u8>, Cow<'a, str>)> {
    paths
        .par_iter()
        .filter_map(|s| {
            let file_name: Vec<u8> = s
                .file_name()?
                .to_string_lossy()
                .to_lowercase()
                .bytes()
                .filter(|b| *b != b' ')
                .collect();
            let full_path = s.to_string_lossy();

            Some((file_name, Cow::Owned(full_path.into_owned())))
        })
        .collect()
}
