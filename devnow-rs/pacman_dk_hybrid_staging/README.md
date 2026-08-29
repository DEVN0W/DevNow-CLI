# Maze Forge Hybrid

Maze Forge Hybrid is a browser game that alternates between two arcade styles inside one run loop:

- a maze-chase phase with pellets, score, lives, power pellets, and roaming ghosts
- a tower-climb phase with jumps, ladders, moving hazards, and a goal at the top

The project uses a tiny Python standard-library server and a canvas-based frontend. No copyrighted game assets are used.

## Run

```bash
python3 server.py
```

Then open `http://127.0.0.1:8011/`.

Optional:

```bash
python3 server.py --host 0.0.0.0 --port 8011
```

## Controls

- Move: arrow keys or `WASD`
- Jump: `Space`
- Restart after game over: `Enter`

## Test

```bash
python3 -m unittest discover -s tests
```
