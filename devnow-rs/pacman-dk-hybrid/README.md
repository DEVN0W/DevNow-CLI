# Maze Climber

Maze Climber is a browser game that blends a pellet-collecting maze chase with a ladder-and-platform escape stage. It uses a Python standard-library server for the backend and vanilla HTML, CSS, and JavaScript for the playable frontend.

## What Was Built

- A Python web server in `app.py` that serves the game and lightweight JSON endpoints.
- A canvas-driven maze stage with pellets, score, lives, and roaming ghosts.
- A platform stage with ladders, jumping, moving hazards, and a goal beacon.
- A hybrid loop that moves from maze to platform stage, then starts a harder cycle.
- Basic `unittest` coverage for the server and entry page.

## Run It

```bash
cd /mnt/c/POC/pacman-dk-hybrid
python3 app.py
```

Then open `http://127.0.0.1:8000` in a browser.

To use a different port:

```bash
PORT=8011 python3 app.py
```

## Test It

```bash
cd /mnt/c/POC/pacman-dk-hybrid
python3 -m unittest discover -s tests
```

## Controls

- Move: arrow keys or `WASD`
- Jump: `Space`
- Start or restart: `Enter`

## Notes

- The artwork is original canvas-drawn geometry and does not reuse copyrighted game assets.
- The implementation is dependency-free because the available runtime does not include `pip` or `pytest`.
