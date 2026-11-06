// CLOCK-VIRTUAL (spec 01 §16): vclock(T) = baseMs + (T − baseTick) × nominalTickMs.
// Mirrors engine::policy::VClock. Clock = {baseMs, baseTick, nominalTickMs} (numbers).
'use strict';

const at = (clock, T) => clock.baseMs + (T - clock.baseTick) * clock.nominalTickMs;

// Switch the rate at tick T, keeping the clock continuous.
const rebase = (clock, T, nominalTickMs) => ({ baseMs: at(clock, T), baseTick: T, nominalTickMs });

module.exports = { at, rebase };
