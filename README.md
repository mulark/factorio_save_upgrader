# Factorio Save Upgrader
Upgrades saves to the current version of Factorio if possible.

NOTICE: uses --start-server to resave the maps. This causes the permissions of
single player saves to get messed up. If you care about that sort of thing please
do not use the tool.

USAGE: ./factorio_save_upgrader *pattern*
upgrades all saves that match the supplied pattern.

Note: Hardcoded to upgrade ~/.factorio/saves/ but can be changed
