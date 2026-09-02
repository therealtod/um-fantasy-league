-- ===========================================================================
-- Canonical reference data: every hero and every board the physical game has
-- printed, as of this migration.
--
-- This is NOT demo data, which is why it lives in `db/migration` next to the
-- schema rather than in `db/seed` with the fixtures. A hero and a board are
-- facts about Unmatched (Restoration Games), not about any one league: an
-- admin cannot price a hero into a tournament pool (`tournament_heroes`) or
-- record a game on a board (`tournament_maps`) that does not exist as a row
-- here first, so a `prod` start with an empty `heroes` table leaves the Admin
-- API with nothing to pool. Every profile therefore gets these rows; only
-- `dev` and `test` go on to add `V3__demo_fixtures.sql` on top.
--
-- Names carry the identity -- there is no slug and no stable external id, and
-- `heroes.name` / `game_maps.name` are unique precisely so the fixtures and the
-- integration tests can join on them. No ids are written here: both tables are
-- empty at this point, so the bigserial assigns them in listed order.
--
-- `heroes.image_url` stays null throughout: no official artwork is bundled, and
-- the UI falls back to a generated placeholder.
--
-- Adding a hero or a board that Restoration Games releases later is a new
-- forward migration in this location, or an admin creating it through
-- `/api/admin/heroes` / `/api/admin/maps` at runtime -- both write the same
-- table.
-- ===========================================================================

-- 74 heroes, in release order.
insert into heroes (name) values
    ('Alice'),
    ('King Arthur'),
    ('Medusa'),
    ('Sinbad'),
    ('Bruce Lee'),
    ('Robin Hood'),
    ('Bigfoot'),
    ('InGen'),
    ('Raptors'),
    ('Sherlock Holmes'),
    ('Invisible Man'),
    ('Dracula'),
    ('Jekyll & Hyde'),
    ('Buffy'),
    ('Angel'),
    ('Spike'),
    ('Willow'),
    ('Little Red Riding Hood'),
    ('Beowulf'),
    ('Deadpool'),
    ('Achilles'),
    ('Yennenga'),
    ('Bloody Mary'),
    ('Sun Wukong'),
    ('Moon Knight'),
    ('Luke Cage'),
    ('Ghost Rider'),
    ('Daredevil'),
    ('Elektra'),
    ('Bullseye'),
    ('Dr. Ellie Sattler'),
    ('T. Rex'),
    ('Houdini'),
    ('The Genie'),
    ('Ms. Marvel'),
    ('Cloak & Dagger'),
    ('Squirrel Girl'),
    ('Black Widow'),
    ('Black Panther'),
    ('Winter Soldier'),
    ('Doctor Strange'),
    ('Spiderman'),
    ('She-Hulk'),
    ('Annie Christmas'),
    ('Golden Bat'),
    ('Nikola Tesla'),
    ('Dr. Jill Trent'),
    ('Tomoe Gozen'),
    ('Oda Nobunaga'),
    ('Shakespeare'),
    ('Titania'),
    ('Hamlet'),
    ('The Wayward Sisters'),
    ('Geralt of Rivia'),
    ('Ancient Leshen'),
    ('Ciri'),
    ('Yennefer & Triss'),
    ('Eredin'),
    ('Philippa'),
    ('Loki'),
    ('Pandora'),
    ('Chupacabra'),
    ('Blackbeard'),
    ('Leonardo'),
    ('Donatello'),
    ('Michelangelo'),
    ('Raphael'),
    ('Shredder'),
    ('Krang'),
    ('Muhammad Ali'),
    ('John Henry'),
    ('Rosie the Riveter'),
    ('George Washington'),
    ('Wyatt Earp');

-- 35 boards, in the same release order as the heroes they shipped with.
insert into game_maps (name) values
    ('Sarpedon'),
    ('Marmoreal'),
    ('Sherwood Forest'),
    ('Yukon'),
    ('Raptor Paddock'),
    ('Soho'),
    ('Baskerville Manor'),
    ('Sunnydale High'),
    ('The Bronze'),
    ('Heorot'),
    ('Hanging Gardens'),
    ('The Raft'),
    ('Hell''s Kitchen'),
    ('T. Rex Paddock'),
    ('King Solomon''s Mine'),
    ('Navy Pier'),
    ('Helicarrier'),
    ('Sanctum Sanctorum'),
    ('McMinnville'),
    ('Point Pleasant'),
    ('Azuchi Castle'),
    ('Globe Theatre'),
    ('Streets of Novigrad'),
    ('Naglfar'),
    ('Kaer Morhen'),
    ('Fayrlund Forest'),
    ('Venice'),
    ('Santa''s Workshop'),
    ('Tsing Shan Monastery'),
    ('Thrilla in Manilla'),
    ('New York City'),
    ('Technodrome'),
    ('The White House'),
    ('The Alamo'),
    ('Yukon-1900');
