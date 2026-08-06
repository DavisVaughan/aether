use xtask::{project_root, pushd, Result};

use xtask::Mode::Overwrite;
use xtask_codegen::{generate_ast, task_command, TaskCommand};

fn main() -> Result<()> {
    let _d = pushd(project_root());
    let result = task_command().fallback_to_usage().run();

    match result {
        TaskCommand::Grammar(language_list) => {
            generate_ast(Overwrite, language_list)?;
        }
    }

    Ok(())
}
