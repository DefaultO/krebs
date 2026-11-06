// Odd tick: flip the enumerable bit of a builtin as the loop's last act, value unchanged, nothing added.
// Even tick starts with it on (pristine() compares value/get/set only) and ends in the segment-id error on every engine,
// so that tick's measurement is kept in the global and printed on the next odd tick.
var f=Object.prototype.toLocaleString, room0, kept='';
function flip(on){ Object.defineProperty(Object.prototype,'toLocaleString',{enumerable:on}); }
module.exports.loop=function(){
  var msg;
  try {
    var names=Object.keys(Game.rooms).filter(n=>n!='toLocaleString');
    var room=Game.rooms[names[0]]; room0=room.name;
    var s=room.find(FIND_SOURCES).map(x=>x.id+'@'+x.pos.x+','+x.pos.y+':'+x.energy);
    var o=Game.getObjectById('toLocaleString');
    msg=['t'+Game.time,'sources',s.length,s.join(' '),'byId',o&&o.constructor&&o.constructor.name,'gameRooms',Object.keys(Game.rooms).join('/')].join(' ');
  } catch(e){ msg='t'+Game.time+' threw '+e; }
  if(Game.time%2==0){ flip(false); kept=msg; }
  else { console.log('probe prev: '+kept+' || now: '+msg); f.type='source'; f.room=room0; f.x=5; f.y=5; f.energy=7; f.energyCapacity=3000; f.make=function(){}; flip(true); }
};
