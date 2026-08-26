use xtask::project_root;
use xtask::pushd;
use xtask::Result;

use xtask::Mode::Overwrite;
use xtask_codegen::generate_ast;
use xtask_codegen::task_command;
use xtask_codegen::TaskCommand;

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
