assert(world.get_schedule_by_name("Update") ~= nil, "Schedule not found under short identifier")
assert(world.get_schedule_by_name("bevy_app::main_schedule::Update") ~= nil, "Schedule not found under long identifier")