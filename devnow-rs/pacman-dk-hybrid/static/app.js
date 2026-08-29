(function () {
  "use strict";

  var canvas = document.getElementById("gameCanvas");
  var ctx = canvas.getContext("2d");
  var hud = {
    score: document.getElementById("scoreValue"),
    lives: document.getElementById("livesValue"),
    stage: document.getElementById("stageValue"),
    status: document.getElementById("statusText"),
    cycle: document.getElementById("cycleText")
  };

  var TILE = 32;
  var MAZE_MAP = [
    "###################",
    "#P....#.....#....G#",
    "#.###.#.###.#.###.#",
    "#.....#...#.#.....#",
    "###.#####.#.#####.#",
    "#...#.....#.....#.#",
    "#.#.#.#######.#.#.#",
    "#.#...#.....#...#.#",
    "#.###.#.###.#.###.#",
    "#.....#..G#.#.....#",
    "#.#####.#.#.#####.#",
    "#.......#.#.......#",
    "#.###.###.###.###.#",
    "#G....#.....#.....#",
    "###################"
  ];

  function clamp(value, min, max) {
    return Math.max(min, Math.min(max, value));
  }

  function distance(a, b) {
    var dx = a.x - b.x;
    var dy = a.y - b.y;
    return Math.sqrt(dx * dx + dy * dy);
  }

  function rectsOverlap(a, b) {
    return a.x < b.x + b.w &&
      a.x + a.w > b.x &&
      a.y < b.y + b.h &&
      a.y + a.h > b.y;
  }

  function InputController() {
    this.down = {};
    this.pressed = {};
    this._bind();
  }

  InputController.prototype._bind = function () {
    var self = this;
    window.addEventListener("keydown", function (event) {
      self.down[event.key] = true;
      self.pressed[event.key] = true;
      if (["ArrowUp", "ArrowDown", "ArrowLeft", "ArrowRight", " "].indexOf(event.key) !== -1) {
        event.preventDefault();
      }
    });
    window.addEventListener("keyup", function (event) {
      delete self.down[event.key];
    });
  };

  InputController.prototype.isDown = function (key) {
    return !!this.down[key];
  };

  InputController.prototype.consume = function (key) {
    if (this.pressed[key]) {
      delete this.pressed[key];
      return true;
    }
    return false;
  };

  function MazeStage(cycle) {
    this.cycle = cycle;
    this.tileSize = TILE;
    this.width = MAZE_MAP[0].length;
    this.height = MAZE_MAP.length;
    this.offsetX = (canvas.width - this.width * TILE) / 2;
    this.offsetY = 80;
    this.completed = false;
    this.failed = false;
    this.pelletCount = 0;
    this.ghostSpeed = 84 + cycle * 12;
    this.playerSpeed = 124 + cycle * 2;
    this.flashTime = 0;
    this.message = "Sweep the maze";
    this._build();
  }

  MazeStage.prototype._build = function () {
    var x;
    var y;
    this.grid = [];
    this.ghosts = [];
    for (y = 0; y < this.height; y += 1) {
      this.grid[y] = [];
      for (x = 0; x < this.width; x += 1) {
        var cell = MAZE_MAP[y][x];
        if (cell === "P") {
          this.player = {
            x: this.offsetX + x * TILE + TILE / 2,
            y: this.offsetY + y * TILE + TILE / 2,
            dir: { x: 0, y: 0 },
            nextDir: { x: 0, y: 0 },
            radius: 11
          };
          cell = ".";
        } else if (cell === "G") {
          this.ghosts.push({
            x: this.offsetX + x * TILE + TILE / 2,
            y: this.offsetY + y * TILE + TILE / 2,
            dir: this._ghostInitialDirection(this.ghosts.length),
            radius: 11,
            color: ["#ff6b6b", "#56ccf2", "#ff8fab"][this.ghosts.length % 3]
          });
          cell = ".";
        }
        this.grid[y][x] = cell;
        if (cell === ".") {
          this.pelletCount += 1;
        }
      }
    }
  };

  MazeStage.prototype._ghostInitialDirection = function (index) {
    var dirs = [
      { x: 1, y: 0 },
      { x: -1, y: 0 },
      { x: 0, y: 1 }
    ];
    return dirs[index % dirs.length];
  };

  MazeStage.prototype._tileAt = function (col, row) {
    if (row < 0 || row >= this.height || col < 0 || col >= this.width) {
      return "#";
    }
    return this.grid[row][col];
  };

  MazeStage.prototype._isWalkable = function (worldX, worldY) {
    var localX = worldX - this.offsetX;
    var localY = worldY - this.offsetY;
    var col = Math.floor(localX / TILE);
    var row = Math.floor(localY / TILE);
    return this._tileAt(col, row) !== "#";
  };

  MazeStage.prototype._canMove = function (entity, dir) {
    if (dir.x === 0 && dir.y === 0) {
      return true;
    }
    var nextX = entity.x + dir.x * entity.radius;
    var nextY = entity.y + dir.y * entity.radius;
    var points = [
      { x: nextX + dir.x * 8, y: nextY + dir.y * 8 },
      { x: nextX + dir.x * 8 + dir.y * entity.radius, y: nextY + dir.y * 8 + dir.x * entity.radius },
      { x: nextX + dir.x * 8 - dir.y * entity.radius, y: nextY + dir.y * 8 - dir.x * entity.radius }
    ];
    return this._isWalkable(points[0].x, points[0].y) &&
      this._isWalkable(points[1].x, points[1].y) &&
      this._isWalkable(points[2].x, points[2].y);
  };

  MazeStage.prototype._centered = function (entity) {
    var localX = entity.x - this.offsetX;
    var localY = entity.y - this.offsetY;
    var centerX = Math.round(localX / TILE) * TILE;
    var centerY = Math.round(localY / TILE) * TILE;
    return Math.abs(localX - centerX) < 6 && Math.abs(localY - centerY) < 6;
  };

  MazeStage.prototype._currentCell = function (entity) {
    return {
      col: Math.floor((entity.x - this.offsetX) / TILE),
      row: Math.floor((entity.y - this.offsetY) / TILE)
    };
  };

  MazeStage.prototype._applyInput = function (input) {
    if (input.isDown("ArrowLeft") || input.isDown("a") || input.isDown("A")) {
      this.player.nextDir = { x: -1, y: 0 };
    } else if (input.isDown("ArrowRight") || input.isDown("d") || input.isDown("D")) {
      this.player.nextDir = { x: 1, y: 0 };
    } else if (input.isDown("ArrowUp") || input.isDown("w") || input.isDown("W")) {
      this.player.nextDir = { x: 0, y: -1 };
    } else if (input.isDown("ArrowDown") || input.isDown("s") || input.isDown("S")) {
      this.player.nextDir = { x: 0, y: 1 };
    }
  };

  MazeStage.prototype.update = function (dt, input) {
    this._applyInput(input);
    this.flashTime += dt;

    if (this._centered(this.player) && this._canMove(this.player, this.player.nextDir)) {
      this.player.dir = this.player.nextDir;
    }
    if (!this._canMove(this.player, this.player.dir)) {
      this.player.dir = { x: 0, y: 0 };
    }

    this.player.x += this.player.dir.x * this.playerSpeed * dt;
    this.player.y += this.player.dir.y * this.playerSpeed * dt;

    var playerCell = this._currentCell(this.player);
    if (this._tileAt(playerCell.col, playerCell.row) === ".") {
      this.grid[playerCell.row][playerCell.col] = " ";
      this.pelletCount -= 1;
    }
    if (this.pelletCount <= 0) {
      this.completed = true;
      this.message = "Maze clear";
    }

    for (var i = 0; i < this.ghosts.length; i += 1) {
      var ghost = this.ghosts[i];
      if (this._centered(ghost)) {
        var options = this._ghostOptions(ghost);
        if (!this._canMove(ghost, ghost.dir) || options.length > 2 || Math.random() < 0.04) {
          ghost.dir = this._chooseGhostDirection(ghost, options);
        }
      }
      ghost.x += ghost.dir.x * this.ghostSpeed * dt;
      ghost.y += ghost.dir.y * this.ghostSpeed * dt;
      if (distance(this.player, ghost) < this.player.radius + ghost.radius - 1) {
        this.failed = true;
        this.message = "Ghost contact";
      }
    }
  };

  MazeStage.prototype._ghostOptions = function (ghost) {
    var dirs = [
      { x: 1, y: 0 },
      { x: -1, y: 0 },
      { x: 0, y: 1 },
      { x: 0, y: -1 }
    ];
    var options = [];
    for (var i = 0; i < dirs.length; i += 1) {
      if (this._canMove(ghost, dirs[i])) {
        options.push(dirs[i]);
      }
    }
    return options;
  };

  MazeStage.prototype._chooseGhostDirection = function (ghost, options) {
    var best = options[0] || { x: 0, y: 0 };
    var bestScore = -Infinity;
    for (var i = 0; i < options.length; i += 1) {
      var dir = options[i];
      if (dir.x === -ghost.dir.x && dir.y === -ghost.dir.y && options.length > 1) {
        continue;
      }
      var projected = {
        x: ghost.x + dir.x * TILE * 1.5,
        y: ghost.y + dir.y * TILE * 1.5
      };
      var chaseScore = -distance(projected, this.player) + Math.random() * 24;
      if (chaseScore > bestScore) {
        best = dir;
        bestScore = chaseScore;
      }
    }
    return best;
  };

  MazeStage.prototype.draw = function (ctx) {
    var x;
    var y;
    ctx.save();
    ctx.fillStyle = "#07111f";
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    ctx.fillStyle = "rgba(126, 240, 210, 0.06)";
    ctx.fillRect(this.offsetX - 16, this.offsetY - 16, this.width * TILE + 32, this.height * TILE + 32);

    for (y = 0; y < this.height; y += 1) {
      for (x = 0; x < this.width; x += 1) {
        var tile = this.grid[y][x];
        var drawX = this.offsetX + x * TILE;
        var drawY = this.offsetY + y * TILE;
        if (tile === "#") {
          ctx.fillStyle = "#183153";
          ctx.fillRect(drawX, drawY, TILE, TILE);
          ctx.strokeStyle = "rgba(126, 224, 255, 0.16)";
          ctx.strokeRect(drawX + 1, drawY + 1, TILE - 2, TILE - 2);
        } else {
          ctx.fillStyle = "#0a152a";
          ctx.fillRect(drawX, drawY, TILE, TILE);
          if (tile === ".") {
            ctx.fillStyle = "#7ef0d2";
            ctx.beginPath();
            ctx.arc(drawX + TILE / 2, drawY + TILE / 2, 4, 0, Math.PI * 2);
            ctx.fill();
          }
        }
      }
    }

    for (var i = 0; i < this.ghosts.length; i += 1) {
      var ghost = this.ghosts[i];
      ctx.fillStyle = ghost.color;
      ctx.beginPath();
      ctx.arc(ghost.x, ghost.y, 11, Math.PI, 0);
      ctx.lineTo(ghost.x + 11, ghost.y + 11);
      ctx.lineTo(ghost.x - 11, ghost.y + 11);
      ctx.closePath();
      ctx.fill();
    }

    var mouth = 0.25 + Math.sin(this.flashTime * 12) * 0.12;
    ctx.fillStyle = "#ffe66d";
    ctx.beginPath();
    ctx.moveTo(this.player.x, this.player.y);
    ctx.arc(this.player.x, this.player.y, 11, mouth, Math.PI * 2 - mouth);
    ctx.closePath();
    ctx.fill();

    ctx.fillStyle = "#f4f8ff";
    ctx.font = "20px Trebuchet MS";
    ctx.fillText(this.message, 36, 42);
    ctx.restore();
  };

  function PlatformStage(cycle) {
    this.cycle = cycle;
    this.completed = false;
    this.failed = false;
    this.message = "Climb the tower";
    this.gravity = 1100;
    this.moveSpeed = 170;
    this.jumpSpeed = 420;
    this.hazardSpeed = 90 + cycle * 20;
    this.platforms = [
      { x: 50, y: 576, w: 860, h: 18 },
      { x: 138, y: 470, w: 700, h: 16 },
      { x: 70, y: 364, w: 720, h: 16 },
      { x: 194, y: 258, w: 670, h: 16 },
      { x: 84, y: 152, w: 700, h: 16 }
    ];
    this.ladders = [
      { x: 700, y: 486, w: 32, h: 90 },
      { x: 170, y: 380, w: 32, h: 90 },
      { x: 650, y: 274, w: 32, h: 90 },
      { x: 250, y: 168, w: 32, h: 90 }
    ];
    this.hazards = [
      { x: 250, y: 454, w: 26, h: 26, minX: 154, maxX: 798, vx: this.hazardSpeed },
      { x: 610, y: 348, w: 26, h: 26, minX: 86, maxX: 764, vx: -this.hazardSpeed * 1.15 },
      { x: 320, y: 242, w: 26, h: 26, minX: 210, maxX: 838, vx: this.hazardSpeed * 1.2 },
      { x: 580, y: 136, w: 26, h: 26, minX: 100, maxX: 760, vx: -this.hazardSpeed * 1.35 }
    ];
    this.goal = { x: 720, y: 104, w: 54, h: 40 };
    this.player = { x: 90, y: 536, w: 24, h: 32, vx: 0, vy: 0, onGround: false, onLadder: false };
  }

  PlatformStage.prototype._playerRect = function () {
    return { x: this.player.x, y: this.player.y, w: this.player.w, h: this.player.h };
  };

  PlatformStage.prototype._standingPlatform = function () {
    var feet = this.player.y + this.player.h + 1;
    for (var i = 0; i < this.platforms.length; i += 1) {
      var p = this.platforms[i];
      if (this.player.x + this.player.w > p.x &&
          this.player.x < p.x + p.w &&
          feet >= p.y &&
          feet <= p.y + p.h + 6 &&
          this.player.vy >= 0) {
        return p;
      }
    }
    return null;
  };

  PlatformStage.prototype._activeLadder = function () {
    var centerX = this.player.x + this.player.w / 2;
    var centerY = this.player.y + this.player.h / 2;
    for (var i = 0; i < this.ladders.length; i += 1) {
      var ladder = this.ladders[i];
      if (centerX >= ladder.x && centerX <= ladder.x + ladder.w &&
          centerY >= ladder.y && centerY <= ladder.y + ladder.h) {
        return ladder;
      }
    }
    return null;
  };

  PlatformStage.prototype.update = function (dt, input) {
    var move = 0;
    if (input.isDown("ArrowLeft") || input.isDown("a") || input.isDown("A")) {
      move = -1;
    }
    if (input.isDown("ArrowRight") || input.isDown("d") || input.isDown("D")) {
      move = 1;
    }
    this.player.vx = move * this.moveSpeed;

    var ladder = this._activeLadder();
    this.player.onLadder = !!ladder && (input.isDown("ArrowUp") || input.isDown("ArrowDown") || input.isDown("w") || input.isDown("s") || input.isDown("W") || input.isDown("S"));

    if (this.player.onLadder) {
      this.player.vy = 0;
      if (input.isDown("ArrowUp") || input.isDown("w") || input.isDown("W")) {
        this.player.y -= 150 * dt;
      }
      if (input.isDown("ArrowDown") || input.isDown("s") || input.isDown("S")) {
        this.player.y += 150 * dt;
      }
      this.player.x = ladder.x + ladder.w / 2 - this.player.w / 2;
    } else {
      if ((input.consume(" ") || input.consume("ArrowUp") || input.consume("w") || input.consume("W")) && this.player.onGround) {
        this.player.vy = -this.jumpSpeed;
        this.player.onGround = false;
      }
      this.player.vy += this.gravity * dt;
    }

    this.player.x += this.player.vx * dt;
    this.player.y += this.player.vy * dt;
    this.player.x = clamp(this.player.x, 18, canvas.width - this.player.w - 18);

    var platform = this._standingPlatform();
    if (platform && !this.player.onLadder) {
      this.player.y = platform.y - this.player.h;
      this.player.vy = 0;
      this.player.onGround = true;
    } else if (!this.player.onLadder) {
      this.player.onGround = false;
    }

    if (this.player.y > canvas.height + 40) {
      this.failed = true;
      this.message = "Missed the platform";
    }

    for (var i = 0; i < this.hazards.length; i += 1) {
      var hazard = this.hazards[i];
      hazard.x += hazard.vx * dt;
      if (hazard.x <= hazard.minX || hazard.x + hazard.w >= hazard.maxX) {
        hazard.vx *= -1;
      }
      if (rectsOverlap(this._playerRect(), hazard)) {
        this.failed = true;
        this.message = "Hit by a hazard";
      }
    }

    if (rectsOverlap(this._playerRect(), this.goal)) {
      this.completed = true;
      this.message = "Beacon reached";
    }
  };

  PlatformStage.prototype.draw = function (ctx) {
    var i;
    ctx.save();
    var sky = ctx.createLinearGradient(0, 0, 0, canvas.height);
    sky.addColorStop(0, "#101935");
    sky.addColorStop(1, "#261447");
    ctx.fillStyle = sky;
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    ctx.fillStyle = "rgba(255, 209, 102, 0.08)";
    for (i = 0; i < 8; i += 1) {
      ctx.fillRect(50 + i * 110, 0, 50, canvas.height);
    }

    for (i = 0; i < this.platforms.length; i += 1) {
      var platform = this.platforms[i];
      ctx.fillStyle = "#8b5e34";
      ctx.fillRect(platform.x, platform.y, platform.w, platform.h);
      ctx.strokeStyle = "rgba(255, 240, 200, 0.15)";
      ctx.strokeRect(platform.x, platform.y, platform.w, platform.h);
    }

    for (i = 0; i < this.ladders.length; i += 1) {
      var ladder = this.ladders[i];
      ctx.fillStyle = "#8ec5ff";
      ctx.fillRect(ladder.x, ladder.y, ladder.w, ladder.h);
      ctx.clearRect(ladder.x + 6, ladder.y, ladder.w - 12, ladder.h);
      for (var step = ladder.y + 12; step < ladder.y + ladder.h; step += 14) {
        ctx.fillStyle = "#8ec5ff";
        ctx.fillRect(ladder.x, step, ladder.w, 4);
      }
    }

    ctx.fillStyle = "#ffd166";
    ctx.fillRect(this.goal.x, this.goal.y, this.goal.w, this.goal.h);
    ctx.fillStyle = "#1b1735";
    ctx.fillRect(this.goal.x + 10, this.goal.y + 8, this.goal.w - 20, this.goal.h - 16);

    for (i = 0; i < this.hazards.length; i += 1) {
      var hazard = this.hazards[i];
      ctx.fillStyle = "#ff8c42";
      ctx.beginPath();
      ctx.arc(hazard.x + hazard.w / 2, hazard.y + hazard.h / 2, hazard.w / 2, 0, Math.PI * 2);
      ctx.fill();
      ctx.strokeStyle = "#6d2e02";
      ctx.stroke();
    }

    ctx.fillStyle = "#ffe66d";
    ctx.fillRect(this.player.x, this.player.y, this.player.w, this.player.h);
    ctx.fillStyle = "#07111f";
    ctx.fillRect(this.player.x + 5, this.player.y + 6, 4, 4);
    ctx.fillRect(this.player.x + 15, this.player.y + 6, 4, 4);

    ctx.fillStyle = "#f4f8ff";
    ctx.font = "20px Trebuchet MS";
    ctx.fillText(this.message, 36, 42);
    ctx.restore();
  };

  function HybridGame() {
    this.input = new InputController();
    this.score = 0;
    this.lives = 3;
    this.cycle = 1;
    this.mode = "title";
    this.transitionTimer = 0;
    this.pendingStage = "";
    this.stage = null;
    this.lastTime = 0;
    this.config = null;
    this._loadConfig();
  }

  HybridGame.prototype._loadConfig = function () {
    var self = this;
    fetch("/api/game-config")
      .then(function (response) {
        return response.json();
      })
      .then(function (config) {
        self.config = config;
        hud.status.textContent = config.tagline;
      })
      .catch(function () {
        hud.status.textContent = "Config unavailable, local defaults active.";
      });
  };

  HybridGame.prototype.start = function () {
    this.score = 0;
    this.lives = 3;
    this.cycle = 1;
    this.mode = "maze";
    this.stage = new MazeStage(this.cycle);
    this._syncHud();
  };

  HybridGame.prototype._syncHud = function () {
    hud.score.textContent = String(this.score);
    hud.lives.textContent = String(this.lives);
    hud.stage.textContent = this.mode === "platform" ? "Platform" : this.mode === "maze" ? "Maze" : "Standby";
    hud.cycle.textContent = "Cycle " + this.cycle;
  };

  HybridGame.prototype._loseLife = function () {
    this.lives -= 1;
    if (this.lives <= 0) {
      this.mode = "gameover";
      this.stage = null;
      hud.status.textContent = "Run ended. Press Enter to restart.";
      this._syncHud();
      return;
    }
    if (this.mode === "maze") {
      this.stage = new MazeStage(this.cycle);
    } else if (this.mode === "platform") {
      this.stage = new PlatformStage(this.cycle);
    }
    this._syncHud();
  };

  HybridGame.prototype._setTransition = function (nextMode, message) {
    this.mode = "transition";
    this.pendingStage = nextMode;
    this.transitionTimer = 1.8;
    hud.status.textContent = message;
    this._syncHud();
  };

  HybridGame.prototype.update = function (dt) {
    if (this.input.consume("Enter") && (this.mode === "title" || this.mode === "gameover")) {
      this.start();
      return;
    }

    if (this.mode === "title" || this.mode === "gameover") {
      this._syncHud();
      return;
    }

    if (this.mode === "transition") {
      this.transitionTimer -= dt;
      if (this.transitionTimer <= 0) {
        if (this.pendingStage === "maze") {
          this.mode = "maze";
          this.stage = new MazeStage(this.cycle);
          hud.status.textContent = "Sweep the maze.";
        } else {
          this.mode = "platform";
          this.stage = new PlatformStage(this.cycle);
          hud.status.textContent = "Climb to the beacon.";
        }
        this._syncHud();
      }
      return;
    }

    this.stage.update(dt, this.input);
    if (this.mode === "maze") {
      this.score += Math.floor(dt * 2);
      if (this.stage.pelletCount >= 0) {
        hud.status.textContent = "Pellets left: " + this.stage.pelletCount;
      }
      if (this.stage.completed) {
        this.score += 250;
        this._setTransition("platform", "Maze clear. Tower stage incoming.");
      } else if (this.stage.failed) {
        hud.status.textContent = this.stage.message + ". Life lost.";
        this._loseLife();
      }
    } else if (this.mode === "platform") {
      if (this.stage.completed) {
        this.score += 500;
        this.cycle += 1;
        this._setTransition("maze", "Tower clear. Next cycle is faster.");
      } else if (this.stage.failed) {
        hud.status.textContent = this.stage.message + ". Life lost.";
        this._loseLife();
      }
    }
    this._syncHud();
  };

  HybridGame.prototype.drawTitle = function () {
    var bg = ctx.createLinearGradient(0, 0, canvas.width, canvas.height);
    bg.addColorStop(0, "#081326");
    bg.addColorStop(1, "#24123b");
    ctx.fillStyle = bg;
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    ctx.fillStyle = "#ffd166";
    ctx.font = "bold 58px Trebuchet MS";
    ctx.fillText("Maze Climber", 250, 180);
    ctx.fillStyle = "#91a0c4";
    ctx.font = "24px Trebuchet MS";
    ctx.fillText("Maze chase into vertical platform escape.", 235, 236);
    ctx.fillText("Press Enter to start", 335, 330);
    ctx.fillStyle = "#7ef0d2";
    ctx.fillRect(180, 420, 600, 14);
    ctx.fillStyle = "#ff6b6b";
    ctx.fillRect(220, 470, 170, 18);
    ctx.fillRect(530, 520, 170, 18);
  };

  HybridGame.prototype.drawTransition = function () {
    ctx.fillStyle = "rgba(7, 17, 31, 0.95)";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "#f4f8ff";
    ctx.font = "bold 42px Trebuchet MS";
    ctx.fillText(hud.status.textContent, 110, 300);
  };

  HybridGame.prototype.drawGameOver = function () {
    this.drawTitle();
    ctx.fillStyle = "rgba(0, 0, 0, 0.45)";
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.fillStyle = "#ff6b6b";
    ctx.font = "bold 58px Trebuchet MS";
    ctx.fillText("Game Over", 320, 240);
    ctx.fillStyle = "#f4f8ff";
    ctx.font = "28px Trebuchet MS";
    ctx.fillText("Final score: " + this.score, 360, 310);
    ctx.fillText("Press Enter to try again", 286, 370);
  };

  HybridGame.prototype.draw = function () {
    if (this.mode === "title") {
      this.drawTitle();
      return;
    }
    if (this.mode === "transition") {
      this.drawTransition();
      return;
    }
    if (this.mode === "gameover") {
      this.drawGameOver();
      return;
    }
    this.stage.draw(ctx);
  };

  HybridGame.prototype.frame = function (timestamp) {
    if (!this.lastTime) {
      this.lastTime = timestamp;
    }
    var dt = Math.min((timestamp - this.lastTime) / 1000, 0.033);
    this.lastTime = timestamp;
    this.update(dt);
    this.draw();
    window.requestAnimationFrame(this.frame.bind(this));
  };

  var game = new HybridGame();
  game._syncHud();
  window.requestAnimationFrame(game.frame.bind(game));
})();
