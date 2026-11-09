#!/bin/sh
# Starts Krebs so that other computers can connect. Use this instead of krebs.
cd "$(dirname "$0")" && exec ./krebs --friends
