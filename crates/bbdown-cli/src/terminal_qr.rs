use std::io::{self, IsTerminal as _, Write as _};
use std::path::Path;

use anyhow::{Context, Result};
use qrcode::{QrCode, render::unicode::Dense1x2};
use terminal_size::{Height, Width};

const BLACK_ON_WHITE: &str = "\x1b[30;47m";
const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TerminalQrOutcome {
    NotInteractive,
    Printed,
    TooSmall,
    GeometryUnavailable,
}

pub(super) fn print_if_supported(code: &QrCode) -> Result<TerminalQrOutcome> {
    let stdout = io::stdout();
    if !stdout.is_terminal() {
        return Ok(TerminalQrOutcome::NotInteractive);
    }

    let choice = anstream::AutoStream::choice(&stdout);
    let color_is_usable = choice != anstream::ColorChoice::Never;
    let Some((Width(columns), Height(rows))) = terminal_size::terminal_size_of(&stdout) else {
        return Ok(TerminalQrOutcome::GeometryUnavailable);
    };
    let Some(image) = render_terminal_qr(code, columns, rows, color_is_usable) else {
        return Ok(TerminalQrOutcome::TooSmall);
    };

    let mut output = anstream::AutoStream::new(stdout, choice).lock();
    for row in image.lines() {
        if color_is_usable {
            writeln!(output, "{BLACK_ON_WHITE}{row}{RESET}")
                .context("failed to write terminal QR code")?;
        } else {
            writeln!(output, "{row}").context("failed to write terminal QR code")?;
        }
    }
    output.flush().context("failed to flush terminal QR code")?;
    Ok(TerminalQrOutcome::Printed)
}

pub(super) fn write_svg_noclobber(code: &QrCode, path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let svg = code
        .render::<qrcode::render::svg::Color<'_>>()
        .quiet_zone(true)
        .build();
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create QR output beside {}", path.display()))?;
    temporary
        .write_all(svg.as_bytes())
        .with_context(|| format!("failed to write QR output for {}", path.display()))?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)
        .with_context(|| {
            format!(
                "failed to create QR output without overwriting {}",
                path.display()
            )
        })?;
    Ok(())
}

fn render_terminal_qr(
    code: &QrCode,
    columns: u16,
    rows: u16,
    color_is_usable: bool,
) -> Option<String> {
    if columns == 0 || rows < 2 {
        return None;
    }

    let image = code.render::<Dense1x2>().quiet_zone(true).build();
    let image = if color_is_usable {
        image
    } else {
        invert_block_glyphs(&image)
    };
    let image_width = image.lines().next()?.chars().count();
    let image_height = image.lines().count();
    if image_width >= usize::from(columns) || image_height.saturating_add(1) >= usize::from(rows) {
        return None;
    }
    Some(image)
}

fn invert_block_glyphs(image: &str) -> String {
    image
        .chars()
        .map(|character| match character {
            '▀' => '▄',
            '▄' => '▀',
            '█' => ' ',
            ' ' => '█',
            other => other,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::{Context, Result};
    use qrcode::{QrCode, render::unicode::Dense1x2};

    use super::{invert_block_glyphs, render_terminal_qr, write_svg_noclobber};

    const PAYLOAD: &str = "https://example.com/scan";

    #[test]
    fn renders_qr_with_quiet_zone_when_terminal_has_room() -> Result<()> {
        let code = QrCode::new(PAYLOAD.as_bytes())?;
        let image = render_terminal_qr(&code, 100, 40, true).context("QR should fit")?;

        let rows: Vec<_> = image.lines().collect();
        assert!(rows.len() > 4);
        assert_eq!(
            rows.first().map(|row| row.chars().count()),
            rows.last().map(|row| row.chars().count())
        );
        assert!(rows[0].chars().all(|character| character == ' '));
        Ok(())
    }

    #[test]
    fn uses_default_terminal_colors_without_ansi() -> Result<()> {
        let code = QrCode::new(PAYLOAD.as_bytes())?;
        let image = render_terminal_qr(&code, 100, 40, false).context("QR should fit")?;
        let expected = invert_block_glyphs(&code.render::<Dense1x2>().quiet_zone(true).build());

        assert_eq!(image, expected);
        assert!(image.contains('█'));
        assert!(
            image
                .lines()
                .next()
                .is_some_and(|row| row.chars().all(|c| c == '█'))
        );
        Ok(())
    }

    #[test]
    fn falls_back_when_terminal_geometry_would_wrap_or_scroll() -> Result<()> {
        let code = QrCode::new(PAYLOAD.as_bytes())?;
        assert!(render_terminal_qr(&code, 20, 40, true).is_none());
        assert!(render_terminal_qr(&code, 100, 5, true).is_none());
        Ok(())
    }

    #[test]
    fn writes_standalone_black_on_white_svg_without_overwriting() -> Result<()> {
        let code = QrCode::new(PAYLOAD.as_bytes())?;
        let directory = tempfile::tempdir()?;
        let path = directory.path().join("scan.svg");
        write_svg_noclobber(&code, &path)?;
        let svg = fs::read_to_string(&path)?;
        assert!(svg.contains("fill=\"#000\""));
        assert!(svg.contains("fill=\"#fff\""));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let metadata = fs::metadata(&path)?;
            assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        }

        let existing_path = directory.path().join("existing.svg");
        fs::write(&existing_path, "preserve existing file")?;
        assert!(write_svg_noclobber(&code, &existing_path).is_err());
        assert_eq!(fs::read_to_string(existing_path)?, "preserve existing file");
        Ok(())
    }
}
