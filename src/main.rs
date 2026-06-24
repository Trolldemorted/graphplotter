use std::error::Error;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use clap::Parser;
use plotters::prelude::*;

#[derive(Parser, Debug)]
#[command(name = "graphplotter", about = "Plot CSV time series to a PNG")]
struct Cli {
    #[arg(required = true)]
    inputs: Vec<PathBuf>,

    #[arg(short = 'o', long = "output")]
    output: PathBuf,

    #[arg(short = 'u', long = "y-unit")]
    y_unit: Option<String>,

    #[arg(long = "since", value_parser = parse_ts)]
    since: Option<DateTime<Utc>>,

    #[arg(long = "until", value_parser = parse_ts)]
    until: Option<DateTime<Utc>>,
}

fn parse_ts(s: &str) -> Result<DateTime<Utc>, String> {
    s.parse::<DateTime<Utc>>().map_err(|e| e.to_string())
}

struct Series {
    name: String,
    points: Vec<(DateTime<Utc>, f64)>,
}

fn series_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("series")
        .to_string()
}

fn read_series(path: &Path) -> Result<Series, Box<dyn Error>> {
    let file = File::open(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let reader = BufReader::new(file);
    let mut points = Vec::new();
    for (idx, line) in reader.lines().enumerate() {
        let line = line.map_err(|e| format!("{}: {}", path.display(), e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let (ts_str, val_str) = trimmed
            .split_once(',')
            .ok_or_else(|| format!("{}:{}: missing comma", path.display(), idx + 1))?;
        let ts = ts_str
            .trim()
            .parse::<DateTime<Utc>>()
            .map_err(|e| format!("{}:{}: bad timestamp: {}", path.display(), idx + 1, e))?;
        let val = val_str
            .trim()
            .parse::<f64>()
            .map_err(|e| format!("{}:{}: bad value: {}", path.display(), idx + 1, e))?;
        points.push((ts, val));
    }
    Ok(Series {
        name: series_name(path),
        points,
    })
}

fn plot(series: &[Series], output: &Path, y_unit: Option<&str>) -> Result<(), Box<dyn Error>> {
    let (mut x_min, mut x_max) = (DateTime::<Utc>::MAX_UTC, DateTime::<Utc>::MIN_UTC);
    let (mut y_min, mut y_max) = (f64::INFINITY, f64::NEG_INFINITY);
    for s in series {
        for &(x, y) in &s.points {
            if x < x_min {
                x_min = x;
            }
            if x > x_max {
                x_max = x;
            }
            if y < y_min {
                y_min = y;
            }
            if y > y_max {
                y_max = y;
            }
        }
    }
    if !y_min.is_finite() || !y_max.is_finite() {
        return Err("no data points to plot".into());
    }
    if x_min == x_max {
        x_max = x_min + chrono::Duration::seconds(1);
    }
    if (y_max - y_min).abs() < f64::EPSILON {
        y_min -= 1.0;
        y_max += 1.0;
    } else {
        let pad = (y_max - y_min) * 0.05;
        y_min -= pad;
        y_max += pad;
    }

    let root = BitMapBackend::new(output, (1280, 720)).into_drawing_area();
    root.fill(&WHITE)?;

    let mut chart = ChartBuilder::on(&root)
        .margin(20)
        .x_label_area_size(50)
        .y_label_area_size(60)
        .caption("graphplotter", ("sans-serif", 28))
        .build_cartesian_2d(x_min..x_max, y_min..y_max)?;

    let mut mesh = chart.configure_mesh();
    mesh.x_label_formatter(&|x: &DateTime<Utc>| x.format("%Y-%m-%d %H:%M:%S").to_string())
        .x_labels(8)
        .y_labels(8)
        .light_line_style(RGBColor(230, 230, 230));
    if let Some(u) = y_unit {
        mesh.y_desc(u);
    }
    mesh.draw()?;

    let palette = [
        RGBColor(31, 119, 180),
        RGBColor(255, 127, 14),
        RGBColor(44, 160, 44),
        RGBColor(214, 39, 40),
        RGBColor(148, 103, 189),
        RGBColor(140, 86, 75),
        RGBColor(227, 119, 194),
        RGBColor(127, 127, 127),
    ];

    for (i, s) in series.iter().enumerate() {
        let color = palette[i % palette.len()];
        chart
            .draw_series(LineSeries::new(
                s.points.iter().copied(),
                color.stroke_width(2),
            ))?
            .label(&s.name)
            .legend(move |(x, y)| {
                PathElement::new(vec![(x, y), (x + 20, y)], color.stroke_width(2))
            });
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    root.present()?;
    Ok(())
}

fn main() {
    let cli = Cli::parse();
    if let (Some(a), Some(b)) = (cli.since, cli.until)
        && a > b
    {
        eprintln!("error: --since ({a}) is after --until ({b})");
        std::process::exit(1);
    }
    let mut all = Vec::with_capacity(cli.inputs.len());
    for path in &cli.inputs {
        match read_series(path) {
            Ok(mut s) => {
                s.points.retain(|(t, _)| {
                    cli.since.is_none_or(|a| *t >= a) && cli.until.is_none_or(|b| *t <= b)
                });
                all.push(s);
            }
            Err(e) => {
                eprintln!("error reading {}: {}", path.display(), e);
                std::process::exit(1);
            }
        }
    }
    if let Err(e) = plot(&all, &cli.output, cli.y_unit.as_deref()) {
        eprintln!("error writing {}: {}", cli.output.display(), e);
        std::process::exit(1);
    }
}
