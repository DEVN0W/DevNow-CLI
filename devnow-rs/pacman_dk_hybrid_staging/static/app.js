const canvas = document.getElementById("game");
const ctx = canvas.getContext("2d");

const WIDTH = canvas.width;
const HEIGHT = canvas.height;
const HUD_HEIGHT = 78;
const COLORS = {
  bg: "#08131f",
  panel: "#0f2130",
  line: "#214765",
  text: "#f6f2df",
  accent: "#f8c44f",
  mint: "#85f0ba",
  danger: "#ff7362",
  blue: "#8ad0ff",
};

const input = {
  left: false,
  right: false,
  up: false,
  down: false,
  jump: false,
  restart: false,
};

const keyMap = {
  ArrowLeft: "left",
  KeyA: "left",
  ArrowRight: "right",
  KeyD: "right",
  ArrowUp: "up",
  KeyW: "up",
  ArrowDown: "down",
  KeyS: "down",
  Space: "jump",
  Enter: "restart",
};

window.addEventListener("keydown", function (event) {
  const action = keyMap[event.code];
  if (!action) {
    return;
  }
  input[action] = true;
  if (["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Space"].indexOf(event.code) >= 0) {
    event.preventDefault();
  }
});

window.addEventListener("keyup", function (event) {
  const action = keyMap[event.code];
  if (!action) {
    return;
  }
  input[action] = false;
});

function clamp(value, min, max) {
  return Math.max(min, Math.min(max, value));
}

function rectsOverlap(a, b) {
  return a.x < b.x + b.w && a.x + a.w > b.x && a.y < b.y + b.h && a.y + a.h > b.y;
}

function distance(ax, ay, bx, by) {
  const dx = ax - bx;
  const dy = ay - by;
  return Math.sqrt(dx * dx + dy * dy);
}

class MazePhase {
  constructor(level) {
    this.level = level;
    this.cell = 32;
    this.originX = 176;
    this.originY = 112;
    this.grid = [
      "###################",
      "#........#........#",
      "#.###.##.#.##.###.#",
      "#o# #.##.#.##.# #o#",
      "#.###.##.#.##.###.#",
      "#.................#",
      "#.###.#.#####.#.###",
      "#.....#...#...#...#",
      "#####.### # ###.###",
      "#   #.#GG   GG#.# #",
      "#####.# ##### #.# #",
      "#........#........#",
      "#.###.##.#.##.###.#",
      "#o..#........#..o.#",
      "###################",
    ];
    this.rows = this.grid.length;
    this.cols = this.grid[0].length;
    this.player = {
      col: 1,
      row: 1,
      x: 0,
      y: 0,
      dir: "right",
      nextDir: "right",
      speed: 7,
      radius: 12,
    };
    this.frightenedTimer = 0;
    this.stepAccumulator = 0;
    this.ghostSpeed = Math.min(6.3, 4.2 + level * 0.38);
    this.ghosts = [];
    this.pellets = {};
    this.pelletCount = 0;
    this._parseGrid();
  }

  _tileKey(col, row) {
    return col + "," + row;
  }

  _parseGrid() {
    for (let row = 0; row < this.rows; row += 1) {
      for (let col = 0; col < this.cols; col += 1) {
        const tile = this.grid[row][col];
        if (tile === "." || tile === "o") {
          this.pellets[this._tileKey(col, row)] = tile;
          this.pelletCount += 1;
        } else if (tile === "G") {
          this.ghosts.push({
            startCol: col,
            startRow: row,
            col: col,
            row: row,
            dir: "left",
            color: ["#ff7362", "#8ad0ff", "#f6f2df", "#ffb347"][this.ghosts.length % 4],
          });
        }
      }
    }
    this._syncPositions();
  }

  _syncPositions() {
    const playerPixel = this.tileCenter(this.player.col, this.player.row);
    this.player.x = playerPixel.x;
    this.player.y = playerPixel.y;
    for (let i = 0; i < this.ghosts.length; i += 1) {
      const ghost = this.ghosts[i];
      const pixel = this.tileCenter(ghost.col, ghost.row);
      ghost.x = pixel.x;
      ghost.y = pixel.y;
    }
  }

  tileCenter(col, row) {
    return {
      x: this.originX + col * this.cell + this.cell / 2,
      y: this.originY + row * this.cell + this.cell / 2,
    };
  }

  tileAt(col, row) {
    if (row < 0 || row >= this.rows || col < 0 || col >= this.cols) {
      return "#";
    }
    return this.grid[row][col];
  }

  canMove(col, row) {
    const tile = this.tileAt(col, row);
    return tile !== "#" && tile !== " ";
  }

  setInput() {
    if (input.left) {
      this.player.nextDir = "left";
    } else if (input.right) {
      this.player.nextDir = "right";
    } else if (input.up) {
      this.player.nextDir = "up";
    } else if (input.down) {
      this.player.nextDir = "down";
    }
  }

  nextTile(col, row, dir) {
    if (dir === "left") {
      return { col: col - 1, row: row };
    }
    if (dir === "right") {
      return { col: col + 1, row: row };
    }
    if (dir === "up") {
      return { col: col, row: row - 1 };
    }
    return { col: col, row: row + 1 };
  }

  moveEntity(entity, dir) {
    const next = this.nextTile(entity.col, entity.row, dir);
    if (this.canMove(next.col, next.row)) {
      entity.col = next.col;
      entity.row = next.row;
      entity.dir = dir;
      const pixel = this.tileCenter(entity.col, entity.row);
      entity.x = pixel.x;
      entity.y = pixel.y;
      return true;
    }
    return false;
  }

  eatPellet(game) {
    const key = this._tileKey(this.player.col, this.player.row);
    if (!this.pellets[key]) {
      return;
    }
    const kind = this.pellets[key];
    delete this.pellets[key];
    this.pelletCount -= 1;
    game.score += kind === "o" ? 50 : 10;
    if (kind === "o") {
      this.frightenedTimer = 6;
    }
  }

  resetGhost(ghost) {
    ghost.col = ghost.startCol;
    ghost.row = ghost.startRow;
    ghost.dir = "left";
    const pixel = this.tileCenter(ghost.col, ghost.row);
    ghost.x = pixel.x;
    ghost.y = pixel.y;
  }

  chooseGhostDirection(ghost) {
    const dirs = ["left", "right", "up", "down"];
    const reverse = {
      left: "right",
      right: "left",
      up: "down",
      down: "up",
    };
    const options = [];
    for (let i = 0; i < dirs.length; i += 1) {
      const dir = dirs[i];
      if (dir === reverse[ghost.dir]) {
        continue;
      }
      const next = this.nextTile(ghost.col, ghost.row, dir);
      if (this.canMove(next.col, next.row)) {
        const playerDistance = Math.abs(this.player.col - next.col) + Math.abs(this.player.row - next.row);
        options.push({ dir: dir, score: playerDistance });
      }
    }
    if (!options.length) {
      return reverse[ghost.dir];
    }
    options.sort(function (a, b) {
      return a.score - b.score;
    });
    if (this.frightenedTimer > 0) {
      options.reverse();
    } else if (Math.random() < 0.18) {
      options.sort(function () {
        return Math.random() - 0.5;
      });
    }
    return options[0].dir;
  }

  update(game, delta) {
    this.setInput();
    this.stepAccumulator += delta * this.player.speed;
    if (this.frightenedTimer > 0) {
      this.frightenedTimer = Math.max(0, this.frightenedTimer - delta);
    }

    while (this.stepAccumulator >= 1) {
      this.stepAccumulator -= 1;
      this.moveEntity(this.player, this.player.nextDir) || this.moveEntity(this.player, this.player.dir);
      this.eatPellet(game);

      for (let i = 0; i < this.ghosts.length; i += 1) {
        const ghost = this.ghosts[i];
        const dir = this.chooseGhostDirection(ghost);
        this.moveEntity(ghost, dir);
      }

      for (let i = 0; i < this.ghosts.length; i += 1) {
        const ghost = this.ghosts[i];
        if (ghost.col === this.player.col && ghost.row === this.player.row) {
          if (this.frightenedTimer > 0) {
            game.score += 200;
            this.resetGhost(ghost);
          } else {
            return { type: "life_lost", reason: "ghost" };
          }
        }
      }

      if (this.pelletCount <= 0) {
        game.score += 250;
        return { type: "cleared" };
      }
    }

    return { type: "running" };
  }

  draw(ctx) {
    ctx.save();
    ctx.fillStyle = "#0b1d2c";
    ctx.fillRect(this.originX - 8, this.originY - 8, this.cols * this.cell + 16, this.rows * this.cell + 16);

    for (let row = 0; row < this.rows; row += 1) {
      for (let col = 0; col < this.cols; col += 1) {
        const x = this.originX + col * this.cell;
        const y = this.originY + row * this.cell;
        const tile = this.grid[row][col];
        if (tile === "#") {
          ctx.fillStyle = "#1d4a6c";
          ctx.fillRect(x + 3, y + 3, this.cell - 6, this.cell - 6);
        } else {
          ctx.strokeStyle = "rgba(110, 170, 220, 0.08)";
          ctx.strokeRect(x, y, this.cell, this.cell);
        }
        const pellet = this.pellets[this._tileKey(col, row)];
        if (pellet) {
          ctx.fillStyle = pellet === "o" ? COLORS.accent : COLORS.text;
          ctx.beginPath();
          ctx.arc(x + this.cell / 2, y + this.cell / 2, pellet === "o" ? 6 : 3, 0, Math.PI * 2);
          ctx.fill();
        }
      }
    }

    ctx.fillStyle = COLORS.accent;
    ctx.beginPath();
    ctx.arc(this.player.x, this.player.y, this.player.radius, 0.2, Math.PI * 1.8);
    ctx.lineTo(this.player.x, this.player.y);
    ctx.fill();

    for (let i = 0; i < this.ghosts.length; i += 1) {
      const ghost = this.ghosts[i];
      ctx.fillStyle = this.frightenedTimer > 0 ? "#5077ff" : ghost.color;
      ctx.beginPath();
      ctx.arc(ghost.x, ghost.y - 4, 12, Math.PI, Math.PI * 2);
      ctx.lineTo(ghost.x + 12, ghost.y + 10);
      ctx.lineTo(ghost.x + 6, ghost.y + 5);
      ctx.lineTo(ghost.x, ghost.y + 10);
      ctx.lineTo(ghost.x - 6, ghost.y + 5);
      ctx.lineTo(ghost.x - 12, ghost.y + 10);
      ctx.closePath();
      ctx.fill();
      ctx.fillStyle = "#08131f";
      ctx.beginPath();
      ctx.arc(ghost.x - 4, ghost.y - 2, 2, 0, Math.PI * 2);
      ctx.arc(ghost.x + 4, ghost.y - 2, 2, 0, Math.PI * 2);
      ctx.fill();
    }

    ctx.restore();
  }
}

class PlatformPhase {
  constructor(level) {
    this.level = level;
    this.gravity = 980;
    this.goal = { x: 804, y: 116, w: 46, h: 46 };
    this.player = {
      x: 120,
      y: 536,
      w: 26,
      h: 34,
      vx: 0,
      vy: 0,
      speed: 205,
      jumpPower: 460,
      climbing: false,
      onGround: false,
    };
    this.platforms = [
      { x: 90, y: 574, w: 780, h: 16 },
      { x: 140, y: 470, w: 680, h: 14 },
      { x: 90, y: 366, w: 760, h: 14 },
      { x: 150, y: 262, w: 640, h: 14 },
      { x: 100, y: 158, w: 760, h: 14 },
    ];
    this.ladders = [
      { x: 208, y: 470, w: 34, h: 104 },
      { x: 672, y: 366, w: 34, h: 104 },
      { x: 280, y: 262, w: 34, h: 104 },
      { x: 630, y: 158, w: 34, h: 104 },
    ];
    const hazardSpeed = 130 + level * 16;
    this.hazards = [
      { x: 200, y: 454, r: 15, vx: hazardSpeed, minX: 160, maxX: 780 },
      { x: 720, y: 350, r: 15, vx: -hazardSpeed * 1.05, minX: 110, maxX: 820 },
      { x: 240, y: 246, r: 15, vx: hazardSpeed * 1.12, minX: 170, maxX: 760 },
      { x: 730, y: 142, r: 15, vx: -hazardSpeed * 1.22, minX: 120, maxX: 850 },
    ];
  }

  ladderAt(player) {
    for (let i = 0; i < this.ladders.length; i += 1) {
      const ladder = this.ladders[i];
      const overlapX = player.x + player.w * 0.5 > ladder.x && player.x + player.w * 0.5 < ladder.x + ladder.w;
      const overlapY = player.y + player.h > ladder.y && player.y < ladder.y + ladder.h;
      if (overlapX && overlapY) {
        return ladder;
      }
    }
    return null;
  }

  update(game, delta) {
    const player = this.player;
    const ladder = this.ladderAt(player);

    player.vx = 0;
    if (input.left) {
      player.vx -= player.speed;
    }
    if (input.right) {
      player.vx += player.speed;
    }

    if (ladder && (input.up || input.down)) {
      player.climbing = true;
      player.vy = 0;
      player.x = ladder.x + ladder.w / 2 - player.w / 2;
    } else if (!ladder) {
      player.climbing = false;
    }

    if (player.climbing) {
      if (input.up) {
        player.y -= 170 * delta;
      }
      if (input.down) {
        player.y += 170 * delta;
      }
      if (input.jump) {
        player.climbing = false;
        player.vy = -player.jumpPower * 0.7;
      }
    } else {
      if (input.jump && player.onGround) {
        player.vy = -player.jumpPower;
        player.onGround = false;
      }
      player.vy += this.gravity * delta;
      player.y += player.vy * delta;
    }

    player.x += player.vx * delta;
    player.x = clamp(player.x, 58, WIDTH - 58 - player.w);

    player.onGround = false;
    for (let i = 0; i < this.platforms.length; i += 1) {
      const platform = this.platforms[i];
      const wasAbove = player.y + player.h - player.vy * delta <= platform.y;
      const overlapsX = player.x + player.w > platform.x && player.x < platform.x + platform.w;
      const lands = !player.climbing && overlapsX && wasAbove && player.y + player.h >= platform.y && player.y + player.h <= platform.y + 30;
      if (lands) {
        player.y = platform.y - player.h;
        player.vy = 0;
        player.onGround = true;
      }
    }

    for (let i = 0; i < this.hazards.length; i += 1) {
      const hazard = this.hazards[i];
      hazard.x += hazard.vx * delta;
      if (hazard.x < hazard.minX || hazard.x > hazard.maxX) {
        hazard.vx *= -1;
        hazard.x = clamp(hazard.x, hazard.minX, hazard.maxX);
      }
      const hit = distance(player.x + player.w / 2, player.y + player.h / 2, hazard.x, hazard.y) < hazard.r + 14;
      if (hit) {
        return { type: "life_lost", reason: "hazard" };
      }
    }

    if (player.y > HEIGHT) {
      return { type: "life_lost", reason: "fall" };
    }

    if (rectsOverlap(player, this.goal)) {
      game.score += 400;
      return { type: "cleared" };
    }

    return { type: "running" };
  }

  draw(ctx) {
    ctx.save();
    const sky = ctx.createLinearGradient(0, HUD_HEIGHT, 0, HEIGHT);
    sky.addColorStop(0, "#102030");
    sky.addColorStop(1, "#251026");
    ctx.fillStyle = sky;
    ctx.fillRect(0, HUD_HEIGHT, WIDTH, HEIGHT - HUD_HEIGHT);

    for (let i = 0; i < this.platforms.length; i += 1) {
      const platform = this.platforms[i];
      ctx.fillStyle = i % 2 === 0 ? "#b86a46" : "#9f5332";
      ctx.fillRect(platform.x, platform.y, platform.w, platform.h);
      ctx.fillStyle = "rgba(0, 0, 0, 0.15)";
      for (let x = platform.x; x < platform.x + platform.w; x += 24) {
        ctx.fillRect(x + 4, platform.y + 3, 14, 4);
      }
    }

    for (let i = 0; i < this.ladders.length; i += 1) {
      const ladder = this.ladders[i];
      ctx.strokeStyle = "#d7c695";
      ctx.lineWidth = 4;
      ctx.beginPath();
      ctx.moveTo(ladder.x + 8, ladder.y);
      ctx.lineTo(ladder.x + 8, ladder.y + ladder.h);
      ctx.moveTo(ladder.x + ladder.w - 8, ladder.y);
      ctx.lineTo(ladder.x + ladder.w - 8, ladder.y + ladder.h);
      ctx.stroke();
      ctx.lineWidth = 3;
      for (let y = ladder.y + 14; y < ladder.y + ladder.h; y += 18) {
        ctx.beginPath();
        ctx.moveTo(ladder.x + 8, y);
        ctx.lineTo(ladder.x + ladder.w - 8, y);
        ctx.stroke();
      }
    }

    ctx.fillStyle = "#7cf3c2";
    ctx.fillRect(this.goal.x, this.goal.y, this.goal.w, this.goal.h);
    ctx.fillStyle = "#163d30";
    ctx.fillRect(this.goal.x + 10, this.goal.y + 10, 26, 26);

    for (let i = 0; i < this.hazards.length; i += 1) {
      const hazard = this.hazards[i];
      ctx.fillStyle = "#ff8d5b";
      ctx.beginPath();
      ctx.arc(hazard.x, hazard.y, hazard.r, 0, Math.PI * 2);
      ctx.fill();
      ctx.fillStyle = "#79341e";
      ctx.beginPath();
      ctx.arc(hazard.x - 4, hazard.y - 3, 3, 0, Math.PI * 2);
      ctx.arc(hazard.x + 4, hazard.y + 3, 3, 0, Math.PI * 2);
      ctx.fill();
    }

    const player = this.player;
    ctx.fillStyle = "#ffd965";
    ctx.fillRect(player.x, player.y, player.w, player.h);
    ctx.fillStyle = "#102030";
    ctx.fillRect(player.x + 5, player.y + 7, 5, 5);
    ctx.fillRect(player.x + 16, player.y + 7, 5, 5);
    ctx.fillStyle = "#4f2630";
    ctx.fillRect(player.x + 4, player.y + player.h - 8, player.w - 8, 6);
    ctx.restore();
  }
}

class HybridGame {
  constructor() {
    this.score = 0;
    this.lives = 3;
    this.level = 1;
    this.mode = "maze";
    this.phase = new MazePhase(this.level);
    this.transitionTimer = 0;
    this.transitionText = "";
    this.flash = 0;
  }

  restart() {
    this.score = 0;
    this.lives = 3;
    this.level = 1;
    this.mode = "maze";
    this.phase = new MazePhase(this.level);
    this.transitionTimer = 0;
    this.transitionText = "";
  }

  setTransition(text, nextMode) {
    this.mode = "transition";
    this.transitionText = text;
    this.nextMode = nextMode;
    this.transitionTimer = 2.1;
  }

  loseLife() {
    this.lives -= 1;
    this.flash = 0.4;
    if (this.lives <= 0) {
      this.mode = "gameover";
      return;
    }
    if (this.phase instanceof MazePhase) {
      this.phase = new MazePhase(this.level);
    } else {
      this.phase = new PlatformPhase(this.level);
    }
  }

  update(delta) {
    if (this.flash > 0) {
      this.flash = Math.max(0, this.flash - delta);
    }

    if (this.mode === "transition") {
      this.transitionTimer -= delta;
      if (this.transitionTimer <= 0) {
        if (this.nextMode === "platform") {
          this.mode = "platform";
          this.phase = new PlatformPhase(this.level);
        } else {
          this.mode = "maze";
          this.phase = new MazePhase(this.level);
        }
      }
      return;
    }

    if (this.mode === "gameover") {
      if (input.restart) {
        this.restart();
      }
      return;
    }

    const result = this.phase.update(this, delta);
    if (result.type === "life_lost") {
      this.loseLife();
      return;
    }
    if (result.type === "cleared") {
      if (this.phase instanceof MazePhase) {
        this.setTransition("Tower run unlocked", "platform");
      } else {
        this.level += 1;
        this.setTransition("Back to the maze", "maze");
      }
    }
  }

  drawHud() {
    ctx.fillStyle = COLORS.panel;
    ctx.fillRect(0, 0, WIDTH, HUD_HEIGHT);
    ctx.strokeStyle = COLORS.line;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(0, HUD_HEIGHT);
    ctx.lineTo(WIDTH, HUD_HEIGHT);
    ctx.stroke();

    ctx.fillStyle = COLORS.accent;
    ctx.font = "bold 26px Trebuchet MS";
    ctx.fillText("Maze Forge Hybrid", 28, 34);

    ctx.fillStyle = COLORS.text;
    ctx.font = "18px Trebuchet MS";
    ctx.fillText("Score: " + this.score, 30, 62);
    ctx.fillText("Lives: " + this.lives, 214, 62);
    ctx.fillText("Loop: " + this.level, 332, 62);

    const phaseLabel = this.phase instanceof MazePhase ? "Maze Chase" : "Tower Climb";
    ctx.fillStyle = COLORS.mint;
    ctx.fillText("Phase: " + phaseLabel, WIDTH - 210, 38);
    ctx.fillStyle = "rgba(246, 242, 223, 0.8)";
    ctx.font = "14px Trebuchet MS";
    ctx.fillText("Clear pellets, then clear the tower to advance.", WIDTH - 296, 60);
  }

  drawOverlay(title, subtitle) {
    ctx.fillStyle = "rgba(5, 9, 17, 0.7)";
    ctx.fillRect(0, 0, WIDTH, HEIGHT);
    ctx.fillStyle = COLORS.accent;
    ctx.font = "bold 42px Trebuchet MS";
    ctx.textAlign = "center";
    ctx.fillText(title, WIDTH / 2, HEIGHT / 2 - 16);
    ctx.fillStyle = COLORS.text;
    ctx.font = "20px Trebuchet MS";
    ctx.fillText(subtitle, WIDTH / 2, HEIGHT / 2 + 24);
    ctx.textAlign = "left";
  }

  draw() {
    ctx.clearRect(0, 0, WIDTH, HEIGHT);
    this.drawHud();
    this.phase.draw(ctx);

    if (this.flash > 0) {
      ctx.fillStyle = "rgba(255, 115, 98, 0.18)";
      ctx.fillRect(0, 0, WIDTH, HEIGHT);
    }

    if (this.mode === "transition") {
      this.drawOverlay(this.transitionText, "Stay alive through both arenas to keep the loop going.");
    } else if (this.mode === "gameover") {
      this.drawOverlay("Run Over", "Press Enter to restart the hybrid run.");
    }
  }
}

const game = new HybridGame();
let lastFrame = performance.now();

function frame(timestamp) {
  const delta = Math.min(0.033, (timestamp - lastFrame) / 1000);
  lastFrame = timestamp;
  game.update(delta);
  game.draw();
  requestAnimationFrame(frame);
}

requestAnimationFrame(frame);
