from pathlib import Path
from PIL import Image

ROOT = Path('/mnt/c/devnow')
SOURCE = ROOT / 'assets/devnow_logo.jpg'
OUTPUTS = [
    ROOT / 'devnow-rs/tui/src/devnow_logo.rs',
    ROOT / 'devnow-rs/tui_app_server/src/devnow_logo.rs',
]

image = Image.open(SOURCE).convert('RGB')
# The left-hand symbol in the normalized 2200x620 master.
symbol = image.crop((54, 44, 410, 526))

def sample(width: int, rows: int):
    resized = symbol.resize((width, rows * 2), Image.Resampling.LANCZOS)
    result = []
    for row in range(rows):
        cells = []
        for column in range(width):
            colors = []
            for y in (row * 2, row * 2 + 1):
                red, green, blue = resized.getpixel((column, y))
                if max(red, green, blue) < 36:
                    red, green, blue = 0, 0, 0
                colors.append((red, green, blue))
            cells.append(tuple(colors))
        result.append(cells)
    return result

def rust_array(name: str, data):
    rows = len(data)
    width = len(data[0])
    lines = [f'pub(crate) const {name}: [[LogoCell; {width}]; {rows}] = [']
    for row in data:
        values = ', '.join(
            f'LogoCell {{ top: ({t[0]}, {t[1]}, {t[2]}), bottom: ({b[0]}, {b[1]}, {b[2]}) }}'
            for t, b in row
        )
        lines.append(f'    [{values}],')
    lines.append('];')
    return '\n'.join(lines)

content = '''#[derive(Clone, Copy)]
pub(crate) struct LogoCell {
    pub(crate) top: (u8, u8, u8),
    pub(crate) bottom: (u8, u8, u8),
}

''' + rust_array('DEVNOW_LOGO_WIDE', sample(12, 8)) + '\n\n' + rust_array('DEVNOW_LOGO_MEDIUM', sample(8, 5)) + '\n'
for output in OUTPUTS:
    output.write_text(content, encoding='utf-8')
