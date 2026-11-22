//! `cellx` — command-line checks for the CellX engines.
//!
//! ```text
//! cellx compat <파일 또는 폴더>...      Excel 저장값과 CellX 계산값 비교
//! cellx gen-data --rows 1000000 -o x.csv  합성 주문 데이터 생성
//! cellx bench-data x.csv                  데이터 시트 + 수식→SQL 성능 측정
//! cellx bench-calc --rows 25000           계산 엔진 성능 측정
//! cellx bench-sheet --rows 100000         큰 시트 작업(저장·열기·정렬·필터…) 측정
//! cellx eval <파일.xlsx> <셀>             파일을 열어 한 셀의 값 확인
//! ```

mod bench;
mod compat;

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cellx", version, about = "CellX 엔진 호환성·성능 점검 도구")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// xlsx 파일의 Excel 저장값과 CellX 재계산값을 셀 단위로 비교합니다.
    Compat {
        /// xlsx 파일 또는 폴더(하위 폴더 포함)
        #[arg(required = true)]
        inputs: Vec<PathBuf>,
        /// 결과를 JSON으로 저장
        #[arg(long)]
        json: Option<PathBuf>,
        /// 파일별 불일치 예시 출력
        #[arg(short, long)]
        verbose: bool,
        /// 일치율이 이 값(%) 미만이면 실패 코드로 종료
        #[arg(long)]
        min_rate: Option<f64>,
        /// 파일 하나당 제한 시간(초)
        #[arg(long, default_value_t = 120)]
        timeout: u64,
    },
    /// (내부용) 파일 하나를 검사해 JSON으로 출력합니다.
    #[command(hide = true)]
    CompatOne { file: PathBuf },
    /// 성능 측정용 합성 주문 데이터(CSV)를 만듭니다.
    GenData {
        #[arg(long, default_value_t = 1_000_000)]
        rows: u64,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// CSV를 데이터 시트로 가져와 수식→SQL 변환 성능을 측정합니다.
    BenchData {
        csv: PathBuf,
        #[arg(long, default_value_t = 5)]
        runs: usize,
    },
    /// 일반 수식으로 채운 시트에서 계산 엔진 성능을 측정합니다.
    BenchCalc {
        #[arg(long, default_value_t = 25_000)]
        rows: i32,
    },
    /// 큰 일반 시트에서 저장·열기·정렬·필터·행 삽입 등 작업 시간을 측정합니다.
    BenchSheet {
        #[arg(long, default_value_t = 100_000)]
        rows: i32,
        /// 측정용 xlsx를 잠시 저장할 폴더
        #[arg(long, default_value = ".")]
        dir: PathBuf,
    },
    /// CSV를 데이터 시트로 넣은 예제 프로젝트(.cellx)를 만듭니다.
    MakeDemo {
        csv: PathBuf,
        #[arg(short, long)]
        out: PathBuf,
    },
    /// xlsx를 열고 다시 계산한 뒤 셀 값을 출력합니다. 예: Sheet1!B3
    Eval { file: PathBuf, cell: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::CompatOne { file } => compat::run_one(&file),
        Command::Compat { inputs, json, verbose, min_rate, timeout } => {
            let report = compat::run(&inputs, std::time::Duration::from_secs(timeout));
            compat::print(&report, verbose);
            if let Some(path) = json {
                std::fs::write(&path, serde_json::to_vec_pretty(&report)?)?;
                println!("\nJSON 저장: {}", path.display());
            }
            if let Some(min) = min_rate {
                if report.match_rate * 100.0 < min {
                    bail!("일치율 {:.2}%가 기준 {min}% 미만입니다", report.match_rate * 100.0);
                }
            }
        }
        Command::GenData { rows, out } => {
            bench::generate(rows, &out)?;
            let size = std::fs::metadata(&out)?.len();
            println!("{} 행, {:.1} MB → {}", rows, size as f64 / 1e6, out.display());
        }
        Command::BenchData { csv, runs } => bench::bench_data(&csv, runs)?,
        Command::BenchCalc { rows } => bench::bench_calc(rows)?,
        Command::BenchSheet { rows, dir } => bench::bench_sheet(rows, &dir)?,
        Command::MakeDemo { csv, out } => bench::make_demo(&csv, &out)?,
        Command::Eval { file, cell } => {
            let mut wb = cellx_core::Workbook::open_xlsx(&file)?;
            wb.recalc()?;
            let (sheet_name, addr) = match cell.split_once('!') {
                Some((s, a)) => (Some(s.trim_matches('\'').to_string()), a.to_string()),
                None => (None, cell.clone()),
            };
            let sheet = match sheet_name {
                Some(n) => wb
                    .sheet_names()
                    .iter()
                    .position(|s| s.eq_ignore_ascii_case(&n))
                    .ok_or_else(|| anyhow::anyhow!("시트가 없습니다: {n}"))? as u32,
                None => 0,
            };
            let Some((row, col)) = cellx_core::bridge::parse_a1(&addr) else {
                bail!("셀 주소가 잘못되었습니다: {addr}");
            };
            let d = wb.detail(sheet, row, col)?;
            println!("입력: {}\n값:   {}", d.input, d.text);
        }
    }
    Ok(())
}
