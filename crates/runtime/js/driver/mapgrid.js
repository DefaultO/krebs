// mirror: reference/driver/lib/runtime/mapgrid.js
// parity: DIVERGENT(J3)
// chitin (J3): vanilla builds the whole world grid per VM from a parsed accessibleRooms list (O(world) per
// player). Here `gridData` is a lazy view over natives.gridData(x, y) (the same t/r/b/l plain-tile counts,
// computed once per TICK-ACCESSIBLE snapshot in Rust, TerrainStore::exit_counts) and `nodes` is built only if
// read (no engine code reads it: map.js uses gridData alone). Entries are memoized for the VM's lifetime, so a
// grid cell, once read, stays stale until the next global reset, as vanilla's whole grid does (TICK-CACHES).
var driver = require('./runtime-driver'),
    C = driver.constants;

// Derived from PathFinding.js (MIT), see THIRD_PARTY_NOTICES.md.
// vanilla: @screeps/pathfinding src/core/Node.js (the only part of that package this file uses).
function Node(x, y, walkable, weight) {
    this.x = x;
    this.y = y;
    this.walkable = (walkable === undefined ? true : walkable);
    this.weight = weight;
}

// vanilla: mapgrid.js:5
function roomNameToXY(name) {

    name = name.toUpperCase();

    var match = name.match(/^(\w)(\d+)(\w)(\d+)$/);
    if(!match) {
        return [undefined, undefined];
    }
    var [,hor,x,ver,y] = match;

    if(hor == 'W') {
        x = -x-1;
    }
    else {
        //x--;
        x = +x;
    }
    if(ver == 'N') {
        y = -y-1;
    }
    else {
        //y--;
        y = +y;
    }
    return [x,y];
};

// vanilla: mapgrid.js:40
function WorldMapGrid(accessibleRooms, staticTerrainData) {
    var grid = this;

    this.width = this.height = driver.getWorldSize();

    // vanilla: `this.nodes = this._buildNodes(this.width, this.height, accessibleRooms)` - lazily.
    var setNodes = (nodes) => Object.defineProperty(grid, 'nodes', {value: nodes, writable: true, enumerable: true, configurable: true});
    Object.defineProperty(this, 'nodes', {
        configurable: true,
        enumerable: true,
        get() {
            var nodes = grid._buildNodes(grid.width, grid.height);
            setNodes(nodes);
            return nodes;
        },
        set: setNodes
    });

    this.gridData = this._buildGridData(
            accessibleRooms, staticTerrainData);
}

// vanilla: mapgrid.js:68 - chitin: a memoizing view keyed `${x},${y}` like vanilla's object; the counts come
// from driver.gridData (accessible rooms only, directions with 0 plain exit tiles omitted, as vanilla).
WorldMapGrid.prototype._buildGridData = function(
        accessibleRooms, staticTerrainData) {
    var cache = Object.create(null);
    return new Proxy(cache, {
        get(target, key) {
            if(typeof key !== 'string' || key in target) {
                return target[key];
            }
            var xy = key.split(',');
            return target[key] = driver.gridData(+xy[0], +xy[1]);
        }
    });
}

// vanilla: mapgrid.js:132 - accessibility per room through natives.isAccessible instead of the list.
WorldMapGrid.prototype._buildNodes = function(width, height) {
    var i, j,
    nodes = {};

    for (i = -height/2; i < height/2; ++i) {
        nodes[i] = {};
        for (j = -width/2; j < width/2; ++j) {
            nodes[i][j] = new Node(j, i);
            var roomName = (j < 0 ? 'W' + (-j - 1) : 'E' + j) + (i < 0 ? 'N' + (-i - 1) : 'S' + i);
            if(driver.natives.isAccessible(roomName)) {
                nodes[i][j].weight = 1;
            }
        }
    }

    return nodes;
};


// vanilla: mapgrid.js:155
WorldMapGrid.prototype.getNodeAt = function(x, y) {
    return this.nodes[y][x];
};


// vanilla: mapgrid.js:167
WorldMapGrid.prototype.isWalkableAt = function(x, y) {
    return this.isInside(x, y) && this.nodes[y][x].weight > 0;
};


// vanilla: mapgrid.js:181
WorldMapGrid.prototype.isInside = function(x, y) {
    return (x >= -this.width/2 && x < this.width/2) && (y >= -this.height/2 && y < this.height/2);
};


// vanilla: mapgrid.js:193
WorldMapGrid.prototype.setWalkableAt = function(x, y, walkable) {

};


// vanilla: mapgrid.js:216
WorldMapGrid.prototype.getNeighbors = function(node, diagonalMovement) {
    var x = node.x,
    y = node.y,
    neighbors = [],
    nodes = this.nodes;


    var gridNodeData = this.gridData[`${x},${y}`];

    if(!gridNodeData) {
        return [];
    }

    // ?
    if (gridNodeData.t && this.isWalkableAt(x,y-1)) {
        neighbors.push(nodes[y - 1][x]);
    }
    // ?
    if (gridNodeData.r && this.isWalkableAt(x+1,y)) {
        neighbors.push(nodes[y][x + 1]);
    }
    // ?
    if (gridNodeData.b && this.isWalkableAt(x,y+1)) {
        neighbors.push(nodes[y + 1][x]);
    }
    // ?
    if (gridNodeData.l && this.isWalkableAt(x-1,y)) {
        neighbors.push(nodes[y][x - 1]);
    }

    return neighbors;
};


// vanilla: mapgrid.js:254
WorldMapGrid.prototype.clone = function() {
    var i, j,

    width = this.width,
    height = this.height,
    thisNodes = this.nodes,

    newGrid = new WorldMapGrid(),
    newNodes = {},
    row;

    for (i = -height/2; i < height/2; ++i) {
        newNodes[i] = {};
        for (j = -width/2; j < width/2; ++j) {
            newNodes[i][j] = new Node(j, i, thisNodes[i][j].walkable, thisNodes[i][j].weight);
        }
    }

    newGrid.nodes = newNodes;

    return newGrid;
};

module.exports = WorldMapGrid;
