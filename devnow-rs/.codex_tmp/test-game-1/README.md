# Neon Run

Neon Run is a small terminal arcade game built with Python standard library only. It uses ANSI rendering and raw terminal keyboard input, so there are no external package dependencies.

## Run

```bash
python3 app.py
```

## Controls

- `Space`: start a run
- `A` / `D` or arrow keys: move left and right
- `R`: restart after a crash
- `Q`: quit

## Test

```bash
python3 -m unittest discover -s tests -p 'test_*.py'
python3 tests/keyboard_probe.py
```
