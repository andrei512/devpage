//! The terminal `devpage` runs in, as the preview's reporter: each problem
//! found while serving is printed on standard error the first time.

use crate::check::Report;
use crate::problem::Problem;

pub struct Terminal;

impl Report for Terminal {
    fn problem(&self, problem: &Problem) {
        eprintln!("{problem}");
    }
}
