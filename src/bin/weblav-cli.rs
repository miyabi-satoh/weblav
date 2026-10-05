//! コマンドプロンプト・ターミナルから使う CLI。中身は `weblav::cli`
//! (本体と exe を分けている理由も同モジュールを参照)。

fn main() {
    if !weblav::cli::handle_args() {
        eprintln!("{}", weblav::cli::USAGE);
        std::process::exit(1);
    }
}
