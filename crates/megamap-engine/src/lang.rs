//! Names shown by the map: entities, biomes, dimensions, items from the game's translations (the glue asks
//! Horizon's `lang` first), with built-in English and Russian tables as the fallback and for the map's own words
//! (weather, time of day, layer modes). `assets/megamap/lang/{en_us,ru_ru}.json` hold the same tables for
//! resource packs and translators; a test keeps them in step.

/// Language of the map's own words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    En,
    Ru,
}

impl Lang {
    /// From a Minecraft language code (`ru_ru`, `en_us`, `uk_ua`...): Russian for ru, else English.
    pub fn from_code(code: &str) -> Lang {
        if code.to_ascii_lowercase().starts_with("ru") { Lang::Ru } else { Lang::En }
    }
    /// The setting (0 follows the game, 1 English, 2 Russian) over the game's code.
    pub fn pick(setting: u8, game_code: &str) -> Lang {
        match setting {
            1 => Lang::En,
            2 => Lang::Ru,
            _ => Lang::from_code(game_code),
        }
    }
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en_us",
            Lang::Ru => "ru_ru",
        }
    }
}

/// Keys of the language setting's values (0 follows the game, 1 English, 2 Russian).
pub const LANGS: [&str; 3] = ["megamap.lang.game", "megamap.lang.en", "megamap.lang.ru"];

/// `(key, English, Russian)`.
pub const TABLE: &[(&str, &str, &str)] = &[
    ("megamap.layer.auto", "Auto", "Авто"),
    ("megamap.layer.surface", "Surface", "Поверхность"),
    ("megamap.layer.cave", "Cave", "Пещеры"),
    ("megamap.layer.fixed_y", "Fixed Y", "Фикс. Y"),
    ("megamap.weather.clear", "Clear", "Ясно"),
    ("megamap.weather.rain", "Rain", "Дождь"),
    ("megamap.weather.thunder", "Thunderstorm", "Гроза"),
    ("megamap.time.morning", "Morning", "Утро"),
    ("megamap.time.day", "Day", "День"),
    ("megamap.time.evening", "Evening", "Вечер"),
    ("megamap.time.night", "Night", "Ночь"),
    ("megamap.day", "Day", "День"),
    ("megamap.light", "Light", "Свет"),
    ("megamap.sky", "sky", "небо"),
    ("megamap.facing", "Facing", "Взгляд"),
    ("megamap.dot", "dot", "точка"),
    ("megamap.hostile", "hostile", "враждебных"),
    ("megamap.hostile_word", "Hostile", "Враждебные"),
    ("megamap.unit.m", "m", "м"),
    ("megamap.color.vanilla", "vanilla", "ванильный"),
    ("megamap.color.pack", "resource pack", "по ресурспаку"),
    ("megamap.color.auto", "auto", "авто"),
    ("dimension.minecraft.overworld", "Overworld", "Верхний мир"),
    ("dimension.minecraft.the_nether", "Nether", "Незер"),
    ("dimension.minecraft.the_end", "The End", "Энд"),
    ("entity.minecraft.player", "Player", "Игрок"),
    ("entity.minecraft.zombie", "Zombie", "Зомби"),
    ("entity.minecraft.skeleton", "Skeleton", "Скелет"),
    ("entity.minecraft.creeper", "Creeper", "Крипер"),
    ("entity.minecraft.spider", "Spider", "Паук"),
    ("entity.minecraft.enderman", "Enderman", "Эндермен"),
    ("entity.minecraft.cow", "Cow", "Корова"),
    ("entity.minecraft.pig", "Pig", "Свинья"),
    ("entity.minecraft.sheep", "Sheep", "Овца"),
    ("entity.minecraft.chicken", "Chicken", "Курица"),
    ("entity.minecraft.villager", "Villager", "Житель"),
    ("entity.minecraft.horse", "Horse", "Лошадь"),
    ("entity.minecraft.wolf", "Wolf", "Волк"),
    ("entity.minecraft.cat", "Cat", "Кошка"),
    ("entity.minecraft.item", "Item", "Предмет"),
    ("biome.minecraft.plains", "Plains", "Равнины"),
    ("biome.minecraft.forest", "Forest", "Лес"),
    ("biome.minecraft.desert", "Desert", "Пустыня"),
    ("biome.minecraft.ocean", "Ocean", "Океан"),
    ("biome.minecraft.river", "River", "Река"),
    ("biome.minecraft.taiga", "Taiga", "Тайга"),
    ("biome.minecraft.savanna", "Savanna", "Саванна"),
    ("biome.minecraft.swamp", "Swamp", "Болото"),
    ("biome.minecraft.jungle", "Jungle", "Джунгли"),
    ("biome.minecraft.beach", "Beach", "Пляж"),
    ("biome.minecraft.nether_wastes", "Nether Wastes", "Пустоши Незера"),
    ("biome.minecraft.the_end", "The End", "Энд"),
];

/// The built-in text for `key`, `None` when the table lacks it.
pub fn builtin(lang: Lang, key: &str) -> Option<&'static str> {
    TABLE.iter().chain(UI.iter()).find(|e| e.0 == key).map(|e| if lang == Lang::Ru { e.2 } else { e.1 })
}

/// The translation key of a registry id: `("entity", "minecraft:zombie")` -> `entity.minecraft.zombie`.
pub fn key_of(kind: &str, id: &str) -> String {
    let (ns, path) = id.split_once(':').unwrap_or(("minecraft", id));
    format!("{kind}.{ns}.{}", path.replace('/', "."))
}

/// A readable name from an id when nothing translates it: `minecraft:dark_oak_forest` -> `Dark oak forest`.
pub fn pretty(id: &str) -> String {
    let p = id.rsplit(':').next().unwrap_or(id).replace('_', " ");
    let mut c = p.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The name of registry id `id` of `kind` (entity, biome, dimension, item): the game's translation (`game`, which
/// returns `None` for a missing key or one in another language than `lang`), else the built-in table, else
/// `pretty`.
pub fn name(lang: Lang, kind: &str, id: &str, game: &dyn Fn(&str) -> Option<String>) -> String {
    let key = key_of(kind, id);
    if let Some(t) = game(&key) {
        return t;
    }
    builtin(lang, &key).map_or_else(|| pretty(id), str::to_string)
}

/// Search match on either the shown name or the id (case-insensitive, `minecraft:` optional).
pub fn matches(query: &str, shown: &str, id: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let id = id.to_lowercase();
    shown.to_lowercase().contains(&q) || id.contains(&q) || id.replace('_', " ").contains(&q)
}

/// Weather name from the rain and thunder levels.
pub fn weather(lang: Lang, rain: f32, thunder: f32) -> &'static str {
    let k = if thunder > 0.5 {
        "megamap.weather.thunder"
    } else if rain > 0.2 {
        "megamap.weather.rain"
    } else {
        "megamap.weather.clear"
    };
    builtin(lang, k).unwrap_or("")
}

/// Time-of-day name for a day time tick (0 = 06:00).
pub fn time_of_day(lang: Lang, tick: u32) -> &'static str {
    let k = match tick % 24000 {
        0..=2999 | 23000..=23999 => "megamap.time.morning",
        3000..=10999 => "megamap.time.day",
        11000..=13499 => "megamap.time.evening",
        _ => "megamap.time.night",
    };
    builtin(lang, k).unwrap_or("")
}

/// The table as a flat JSON object for one language (what `assets/megamap/lang/*.json` holds).
pub fn to_json(lang: Lang) -> String {
    let mut s = String::from("{\n");
    let n = TABLE.len() + UI.len();
    for (i, e) in TABLE.iter().chain(UI.iter()).enumerate() {
        let v = if lang == Lang::Ru { e.2 } else { e.1 };
        s += &format!("  \"{}\": \"{}\"{}\n", e.0, v.replace('"', "\\\""), if i + 1 < n { "," } else { "" });
    }
    s + "}\n"
}

/// The map's interface words: settings items and values, categories, hints, buttons, toasts. `{0}`, `{1}` are
/// filled by `Tr::f`.
pub const UI: &[(&str, &str, &str)] = &[
    // Common values.
    ("megamap.on", "on", "вкл"),
    ("megamap.off", "off", "выкл"),
    ("megamap.yes", "yes", "да"),
    ("megamap.no", "no", "нет"),
    ("megamap.v.as_game", "as game", "как в игре"),
    ("megamap.v.auto", "auto", "авто"),
    ("megamap.v.none", "none", "нет"),
    ("megamap.v.px", "{0} px", "{0} px"),
    ("megamap.v.blocks", "{0} blocks", "{0} бл."),
    ("megamap.v.chunks_blocks", "{0} chunks ({1} blocks)", "{0} чанк. ({1} бл.)"),
    ("megamap.v.every", "every {0}", "каждые {0}"),
    ("megamap.v.unit_chunks", "chunks", "чанки"),
    ("megamap.v.unit_blocks", "blocks", "блоки"),
    ("megamap.v.seconds", "{0} s", "{0} с"),
    ("megamap.v.round", "round", "круглая"),
    ("megamap.v.square", "square", "квадрат"),
    ("megamap.v.shown", "shown", "видна"),
    ("megamap.v.hidden", "hidden", "скрыта"),
    ("megamap.v.y", "Y {0}", "Y {0}"),
    ("megamap.v.off_roof", "off", "выкл"),
    ("megamap.v.roof", "{0}x{0} columns", "{0}x{0} колонн"),
    ("megamap.v.indoor_cut", "Cut roof", "Срез крыши"),
    ("megamap.v.reveal_0", "At once", "Сразу"),
    ("megamap.v.reveal_1", "Slow", "Медленно"),
    ("megamap.v.reveal_2", "Normal", "Обычно"),
    ("megamap.v.reveal_3", "Fast", "Быстро"),
    ("megamap.v.badge_0", "Icon", "Значок"),
    ("megamap.v.badge_1", "Text", "Текст"),
    ("megamap.v.badge_2", "Off", "Нет"),
    ("megamap.v.hostile_0", "Icon and number", "Значок и число"),
    ("megamap.v.hostile_1", "Text", "Текст"),
    ("megamap.v.hostile_2", "Off", "Нет"),
    ("megamap.v.info_align_0", "Centre", "По центру"),
    ("megamap.v.info_align_1", "Left", "Слева"),
    ("megamap.v.info_align_2", "Right", "Справа"),
    ("megamap.v.indoor_zoom", "Cut, zoom x{0}", "Срез, зум x{0}"),
    ("megamap.corner.top_left", "top left", "слева сверху"),
    ("megamap.corner.top_right", "top right", "справа сверху"),
    ("megamap.corner.bottom_left", "bottom left", "слева снизу"),
    ("megamap.corner.bottom_right", "bottom right", "справа снизу"),
    ("megamap.relief.0", "flat", "плоско"),
    ("megamap.relief.1", "soft", "мягко"),
    ("megamap.relief.2", "normal", "обычно"),
    ("megamap.relief.3", "strong", "сильно"),
    ("megamap.grid.0", "off", "выкл"),
    ("megamap.grid.1", "chunks", "чанки"),
    ("megamap.grid.2", "regions", "регионы"),
    ("megamap.grid.3", "both", "оба"),
    ("megamap.theme.auto", "auto", "авто"),
    ("megamap.theme.day", "day", "день"),
    ("megamap.theme.sunset", "sunset", "закат"),
    ("megamap.theme.night", "night", "ночь"),
    ("megamap.theme.storm", "storm", "гроза"),
    ("megamap.quality.0", "fast", "быстро"),
    ("megamap.quality.1", "normal", "обычно"),
    ("megamap.quality.2", "fine", "детально"),
    ("megamap.quality.3", "volumetric", "объёмно"),
    ("megamap.cloud_dim.0", "follow map", "по карте"),
    ("megamap.cloud_dim.1", "Overworld", "Верхний мир"),
    ("megamap.cloud_dim.2", "Nether", "Незер"),
    ("megamap.cloud_dim.3", "End", "Энд"),
    ("megamap.lightning.0", "off", "выкл"),
    ("megamap.lightning.1", "flashes", "вспышки"),
    ("megamap.lightning.2", "flashes + bolts", "вспышки + разряды"),
    ("megamap.ui_theme.0", "auto", "авто"),
    ("megamap.ui_theme.1", "dark", "тёмная"),
    ("megamap.ui_theme.2", "light", "светлая"),
    ("megamap.ent_mode.0", "dot", "точка"),
    ("megamap.ent_mode.1", "face", "лицо"),
    ("megamap.ent_mode.2", "3D sprite", "3D-спрайт"),
    ("megamap.surface_rule.0", "sky or height", "небо или высота"),
    ("megamap.surface_rule.1", "sky only", "только небо"),
    ("megamap.surface_rule.2", "height only", "только высота"),
    ("megamap.lang.game", "as game", "как в игре"),
    ("megamap.lang.en", "English", "English"),
    ("megamap.lang.ru", "Русский", "Русский"),
    ("megamap.info.coords", "Coordinates", "Координаты"),
    ("megamap.info.biome", "Biome", "Биом"),
    ("megamap.info.time", "Time", "Время"),
    ("megamap.info.light", "Light level", "Освещённость"),
    ("megamap.info.facing", "Facing", "Направление"),
    ("megamap.info.dimension", "Dimension", "Измерение"),
    ("megamap.ent_cat.hostile", "Hostile", "Враждебные"),
    ("megamap.ent_cat.passive", "Passive", "Мирные"),
    ("megamap.ent_cat.players", "Players", "Игроки"),
    ("megamap.ent_cat.other", "Other", "Прочие"),
    // Settings items.
    ("megamap.set.shape", "Shape", "Форма"),
    ("megamap.set.north_lock", "Lock north", "Север сверху"),
    ("megamap.set.size", "Size", "Размер"),
    ("megamap.set.zoom", "Zoom", "Масштаб"),
    ("megamap.set.mode", "Minimap", "Миникарта"),
    ("megamap.set.corner", "Corner", "Угол"),
    ("megamap.set.opacity", "Opacity", "Непрозрачность"),
    ("megamap.set.compass", "Compass", "Компас"),
    ("megamap.set.mini_fps", "Minimap FPS", "FPS миникарты"),
    ("megamap.set.radar", "Entity radar", "Радар сущностей"),
    ("megamap.set.beams", "Waypoint beams", "Лучи вейпоинтов"),
    ("megamap.set.trail", "Trail", "След"),
    ("megamap.set.deaths", "Death points kept", "Точек смерти"),
    ("megamap.set.info", "Line {0}: {1}", "Строка {0}: {1}"),
    ("megamap.set.map_fps", "Map FPS", "FPS карты"),
    ("megamap.set.fog", "Exploration fog", "Туман исследования"),
    ("megamap.set.relief", "Relief shading", "Тени рельефа"),
    ("megamap.set.contours", "Contour lines", "Изолинии"),
    ("megamap.set.grid", "Grid", "Сетка"),
    ("megamap.set.day_night", "Day / night light", "День и ночь"),
    ("megamap.set.far", "Far terrain (LOD)", "Дальний ландшафт"),
    ("megamap.set.clouds", "Cloud fog", "Облачный туман"),
    ("megamap.set.cloud_density", "Cloud density", "Плотность облаков"),
    ("megamap.set.cloud_speed", "Cloud speed", "Скорость облаков"),
    ("megamap.set.cloud_puff", "Cloud puffiness", "Пышность облаков"),
    ("megamap.set.cloud_theme", "Cloud theme", "Тема облаков"),
    ("megamap.set.cloud_weather", "Clouds follow weather", "Облака по погоде"),
    ("megamap.set.cloud_quality", "Cloud quality", "Качество облаков"),
    ("megamap.set.cloud_fps", "Cloud FPS limit", "Предел FPS облаков"),
    ("megamap.set.cloud_dim", "Cloud look", "Вид облаков"),
    ("megamap.set.nether", "Nether ash", "Пепел Незера"),
    ("megamap.set.end", "End haze", "Дымка Энда"),
    ("megamap.set.lightning", "Lightning", "Молнии"),
    ("megamap.set.lightning_rate", "Lightning rate", "Частота молний"),
    ("megamap.set.god_rays", "God rays", "Лучи света"),
    ("megamap.set.ui_theme", "UI theme", "Тема интерфейса"),
    ("megamap.set.animations", "Animations", "Анимации"),
    ("megamap.set.map_reach", "Cloud reach (map)", "Заход облаков, карта"),
    ("megamap.set.far_threads", "Far LOD readers", "Потоки LOD"),
    ("megamap.set.reset_all", "Reset all to defaults", "Сбросить все настройки"),
    ("megamap.set.autozoom", "Auto-zoom", "Авто-масштаб"),
    ("megamap.set.az_near", "Closest zoom", "Макс. приближение"),
    ("megamap.set.az_far", "Farthest zoom", "Макс. отдаление"),
    ("megamap.set.ent_mode", "{0}", "{0}"),
    ("megamap.set.face_frame", "Icon frame", "Рамка иконок"),
    ("megamap.set.layer", "Layer", "Слой"),
    ("megamap.set.fixed_y", "Fixed layer top", "Верх фикс. слоя"),
    ("megamap.set.cave_depth", "Cave slice depth", "Глубина среза"),
    ("megamap.set.surface_rule", "Surface when", "Поверхность, если"),
    ("megamap.set.ui_fixed", "Fixed UI look", "Постоянный вид UI"),
    ("megamap.set.color_style", "Block colours", "Цвета блоков"),
    ("megamap.set.lang", "Map language", "Язык карты"),
    ("megamap.set.reach_unit", "Reach unit", "Единицы захода"),
    ("megamap.set.mini_reach", "Cloud reach (minimap)", "Заход облаков, мини"),
    ("megamap.set.roof_size", "Roof check", "Проверка потолка"),
    ("megamap.set.roof_reach", "Roof search height", "Поиск потолка"),
    ("megamap.set.layer_delay", "Switch delay", "Задержка смены"),
    ("megamap.set.cave_step", "Cave layer step", "Шаг слоя пещер"),
    ("megamap.set.indoor", "Indoors", "В здании"),
    ("megamap.set.smooth", "Smooth movement", "Плавное движение"),
    ("megamap.set.textures", "Block textures", "Текстуры блоков"),
    ("megamap.set.block_light", "Light sources", "Источники света"),
    ("megamap.set.reveal", "Fog retreat", "Отступление тумана"),
    ("megamap.set.far_fog", "Fog over far terrain", "Туман над дальним ландшафтом"),
    ("megamap.set.badge", "Layer badge", "Значок слоя"),
    ("megamap.set.hostile", "Hostile warning", "Предупреждение о враждебных"),
    ("megamap.set.info_align", "Info alignment", "Выравнивание инфо"),
    // Categories, groups, tabs.
    ("megamap.tab.minimap", "Minimap", "Миникарта"),
    ("megamap.tab.map", "World map", "Карта мира"),
    ("megamap.cat.view", "View", "Вид"),
    ("megamap.cat.info", "Info", "Информация"),
    ("megamap.cat.entities", "Entities", "Сущности"),
    ("megamap.cat.autozoom", "Auto-zoom", "Авто-масштаб"),
    ("megamap.cat.clouds", "Clouds & fog", "Облака, туман"),
    ("megamap.cat.perf", "Performance", "Скорость"),
    ("megamap.cat.waypoints", "Waypoints", "Вейпоинты"),
    ("megamap.cat.layers", "Layers", "Слои"),
    ("megamap.cat.data", "Data / LOD", "Данные, LOD"),
    ("megamap.cat.ui", "Interface", "Интерфейс"),
    ("megamap.group.shape", "Shape and place", "Форма и место"),
    ("megamap.group.orient", "Orientation", "Ориентация"),
    ("megamap.group.layers", "Layers", "Слои"),
    ("megamap.group.auto_layer", "Auto layer", "Авто-слой"),
    ("megamap.group.info_lines", "Lines under the map", "Строки под картой"),
    ("megamap.group.marks", "Marks", "Метки"),
    ("megamap.group.radar", "Radar", "Радар"),
    ("megamap.group.display", "Display", "Отображение"),
    ("megamap.group.autozoom", "Auto-zoom", "Авто-масштаб"),
    ("megamap.group.fog", "Fog", "Туман"),
    ("megamap.group.clouds", "Clouds", "Облака"),
    ("megamap.group.fps", "Frame rate", "Частота кадров"),
    ("megamap.group.quality", "Quality", "Качество"),
    ("megamap.group.relief", "Relief", "Рельеф"),
    ("megamap.group.grid_light", "Grid and light", "Сетка и свет"),
    ("megamap.group.colors", "Colours", "Цвета"),
    ("megamap.group.waypoints", "Waypoints", "Вейпоинты"),
    ("megamap.group.trail", "Trail", "След"),
    ("megamap.group.explore_fog", "Exploration fog", "Туман исследования"),
    ("megamap.group.dims", "Dimensions", "Измерения"),
    ("megamap.group.storm", "Storm", "Гроза"),
    ("megamap.group.far", "Far terrain", "Дальний ландшафт"),
    ("megamap.group.look", "Look", "Оформление"),
    ("megamap.group.reset", "Reset", "Сброс"),
    // Hints.
    ("megamap.hint.shape", "Round or square minimap", "Круглая или квадратная миникарта"),
    ("megamap.hint.north_lock", "North always up, or the map turns with the player", "Север всегда сверху или карта вращается с игроком"),
    ("megamap.hint.size", "Minimap size on screen", "Размер миникарты на экране"),
    ("megamap.hint.zoom", "Minimap scale (Z); with auto-zoom a factor on it", "Масштаб миникарты (Z); при авто-масштабе - множитель к нему"),
    ("megamap.hint.mode", "Show or hide the minimap (the layer is set under Layers)", "Показать или скрыть миникарту (слой выбирается в «Слои»)"),
    ("megamap.hint.corner", "Screen corner of the minimap", "Угол экрана для миникарты"),
    ("megamap.hint.opacity", "Opacity of the map image (the frame stays opaque)", "Прозрачность изображения карты (рамка всегда непрозрачна)"),
    ("megamap.hint.compass", "Compass letters around the minimap", "Буквы сторон света вокруг миникарты"),
    ("megamap.hint.mini_fps", "Limit of minimap redraws per second", "Ограничение перерисовок миникарты в секунду"),
    ("megamap.hint.radar", "Show entities near the player", "Показывать сущности рядом с игроком"),
    ("megamap.hint.beams", "Waypoint light beams in the world", "Световые столбы вейпоинтов в мире"),
    ("megamap.hint.trail", "Trail of the path walked", "След пройденного пути"),
    ("megamap.hint.deaths", "How many death points to keep", "Сколько точек смерти хранить"),
    ("megamap.hint.info", "Info line under the minimap; the arrows change the order", "Строка информации под миникартой; стрелки меняют порядок"),
    ("megamap.hint.map_fps", "Limit of world map redraws per second", "Ограничение перерисовок карты мира в секунду"),
    ("megamap.hint.fog", "Hide the unexplored under fog", "Скрывать неисследованное туманом"),
    ("megamap.hint.relief", "Strength of the relief shading", "Сила затенения рельефа"),
    ("megamap.hint.contours", "Height contour lines", "Изолинии высоты"),
    ("megamap.hint.grid", "Chunk and region grid", "Сетка чанков и регионов"),
    ("megamap.hint.day_night", "The map darkens at night", "Карта темнеет ночью"),
    ("megamap.hint.far", "Far terrain from Horizon's LOD data past the loaded chunks", "Дальний ландшафт из LOD-данных Horizon за пределами загруженных чанков"),
    ("megamap.hint.clouds", "The fog as animated clouds", "Туман в виде анимированных облаков"),
    ("megamap.hint.cloud_density", "How much of the unexplored the clouds cover", "Сколько неисследованного закрыто облаками"),
    ("megamap.hint.cloud_speed", "Cloud drift speed", "Скорость дрейфа облаков"),
    ("megamap.hint.cloud_puff", "Flat layers or puffy cumulus", "Плоские слои или пышные кучевые облака"),
    ("megamap.hint.cloud_theme", "Cloud colours: by time of day and weather, or fixed", "Цвета облаков: по времени суток и погоде или фиксированные"),
    ("megamap.hint.cloud_weather", "Clouds darken and hurry in rain and storms", "Облака темнеют и ускоряются в дождь и грозу"),
    ("megamap.hint.cloud_quality", "Cloud detail (fast to volumetric)", "Детализация облаков (быстро - объёмные)"),
    ("megamap.hint.cloud_fps", "Frame rate of the cloud animation", "Частота кадров анимации облаков"),
    ("megamap.hint.cloud_dim", "Cloud look: by the map's dimension, or fixed", "Вид облаков: по измерению карты или фиксированный"),
    ("megamap.hint.nether", "Strength of the Nether ash", "Сила пепла Незера"),
    ("megamap.hint.end", "Strength of the End haze", "Сила дымки Энда"),
    ("megamap.hint.lightning", "Lightning in storms", "Молнии в грозу"),
    ("megamap.hint.lightning_rate", "How often lightning strikes", "Частота молний"),
    ("megamap.hint.god_rays", "Sun or moon rays through cloud gaps", "Лучи солнца или луны через разрывы облаков"),
    ("megamap.hint.ui_theme", "Light or dark interface theme", "Светлая или тёмная тема интерфейса"),
    ("megamap.hint.animations", "Smooth opening of the map", "Плавное открытие карты"),
    ("megamap.hint.map_reach", "How far (in world blocks) the clouds reach over explored land on the map: a hard limit, the soft edge fits inside it", "Насколько далеко (в блоках мира) облака заходят на исследованную землю на карте: жёсткий предел, мягкий край внутри него"),
    ("megamap.hint.far_threads", "How many threads read far terrain from disk: auto = a quarter of the cores, at most 3", "Сколько потоков читают дальний ландшафт с диска: авто = четверть ядер, не больше 3"),
    ("megamap.hint.reset_all", "Reset every setting", "Сбросить все настройки"),
    ("megamap.hint.autozoom", "The minimap scale follows the radius of known data; Z and the wheel are a factor", "Масштаб миникарты сам подбирается по радиусу известных данных; Z и колесо - множитель"),
    ("megamap.hint.az_near", "Auto-zoom never zooms in closer than this", "Ближе этого авто-масштаб не приблизит"),
    ("megamap.hint.az_far", "Auto-zoom never zooms out farther than this", "Дальше этого авто-масштаб не отдалит"),
    ("megamap.hint.ent_mode", "Dot, face (from the skin or mob texture) or 3D sprite of the model", "Точка, лицо (из скина или текстуры моба) или 3D-спрайт модели"),
    ("megamap.hint.face_frame", "Width of the coloured frame around faces and sprites (colour by category), 0 = none", "Толщина цветной рамки вокруг лиц и спрайтов (цвет по категории), 0 - без рамки"),
    ("megamap.hint.layer", "Auto: surface under the open sky, cave slice under ground; or a fixed layer (the map toolbar button cycles it too)", "Авто: поверхность под открытым небом, срез пещеры под землёй; или фиксированный слой (кнопка на панели карты тоже переключает)"),
    ("megamap.hint.fixed_y", "Top of the slice for the Fixed Y layer", "Верх среза для слоя «Фикс. Y»"),
    ("megamap.hint.cave_depth", "How many blocks below the slice top the cave layer shows (ceilings are cut off)", "Сколько блоков вниз от верха среза видно в пещерном слое (потолки срезаются)"),
    ("megamap.hint.surface_rule", "What tells auto the player is on the surface: sky light, height above the ground, or either", "По какому признаку авто считает, что игрок на поверхности: свет неба, высота над рельефом или любое из двух"),
    ("megamap.hint.ui_fixed", "Panels, frame, theme and tint ignore time of day and weather (the map's own day and night is a separate setting)", "Панели, рамка, тема и подсветка интерфейса не меняются от времени суток и погоды (день и ночь самой карты - отдельная настройка)"),
    ("megamap.hint.color_style", "Colours of blocks, water, leaves and grass: vanilla, from the resource pack textures, or auto (the pack when one is on)", "Цвета блоков, воды, листвы и травы: ванильные, из текстур ресурспака или авто (ресурспак, если он включён)"),
    ("megamap.hint.lang", "Language of the map's words: as the game, English or Russian", "Язык надписей карты: как в игре, английский или русский"),
    ("megamap.hint.reach_unit", "Set the cloud reach in blocks or chunks (16-block steps, the limit rounds down to whole chunks)", "Задавать заход облаков в блоках или чанках (шаг 16 блоков, предел округляется вниз до целых чанков)"),
    ("megamap.hint.mini_reach", "How far (in world blocks) the clouds reach over explored land on the minimap: a hard limit", "Насколько далеко (в блоках мира) облака заходят на исследованную землю на миникарте: жёсткий предел"),
    ("megamap.hint.roof_size", "Auto layer: how many columns above the player must have a solid roof (3x3 ignores a single log or a bridge edge)", "Авто-слой: сколько колонн над игроком должны иметь твёрдый потолок (3x3 не реагирует на одно бревно или край моста)"),
    ("megamap.hint.roof_reach", "Auto layer: how high above the head to look for the roof", "Авто-слой: как высоко над головой искать потолок"),
    ("megamap.hint.layer_delay", "Auto layer: how long the new place must hold before the layer switches (no flicker at cave mouths)", "Авто-слой: сколько новое место должно держаться, прежде чем слой сменится (без мигания у входа в пещеру)"),
    ("megamap.hint.smooth", "Pixels glide instead of jumping when you walk or turn", "Пиксели плавно скользят, а не прыгают при ходьбе и повороте"),
    ("megamap.hint.textures", "Zoomed in close, blocks show their textures", "При сильном приближении у блоков видны текстуры"),
    ("megamap.hint.far_fog", "Cover far (estimated, LOD) terrain with the exploration clouds too. Off: far terrain stays visible and the clouds only hide ground the map knows nothing about", "Закрывать облаками и дальний (примерный, LOD) ландшафт. Выкл.: дальний ландшафт виден, облака скрывают только места, о которых карта ничего не знает"),
    ("megamap.hint.hostile", "A red line under the minimap when hostile mobs are within radar range: a skull with how many and how far the nearest is, the same in words, or nothing", "Красная строка под миникартой, когда враждебные мобы в зоне радара: череп, их число и расстояние до ближайшего, то же словами, или ничего"),
    ("megamap.hint.info_align", "Where the lines under the minimap line up: centred under it, along its left edge or along its right edge", "Как выровнены строки под миникартой: по центру, по левому или по правому краю"),
    ("megamap.hint.badge", "What the minimap corner shows when the map leaves the surface: an icon (surface, cave, building), a word, or nothing. The heights are in the map's title line", "Что показывать в углу миникарты, когда карта уходит с поверхности: значок (поверхность, пещера, здание), слово или ничего. Высоты указаны в заголовке большой карты"),
    ("megamap.hint.reveal", "How fast the fog edge moves back from you over newly explored ground; At once shows it with no animation", "Как быстро край тумана отходит от вас над только что разведанной местностью; «Сразу» показывает её без анимации"),
    ("megamap.hint.block_light", "Torches, lava and other light sources glow on the map, most at night", "Факелы, лава и другие источники света светятся на карте, сильнее ночью"),
    ("megamap.hint.indoor", "Under a roof (a house, a room): the map cuts the ceiling to show the floor you are on, and the minimap zooms in", "Под крышей (дом, комната): карта срезает потолок, чтобы был виден твой этаж, а миникарта приближается"),
    ("megamap.hint.cave_step", "Cave slice moves with the player in steps of this many blocks", "Срез пещер следует за игроком шагами по столько блоков"),
    // Panels and the world map screen.
    ("megamap.ui.search", "Search...", "Поиск..."),
    ("megamap.ui.search_settings", "Search settings...", "Поиск по настройкам..."),
    ("megamap.ui.no_waypoints", "No waypoints", "Нет вейпоинтов"),
    ("megamap.ui.new_wp", "+ New waypoint here", "+ Новый вейпоинт здесь"),
    ("megamap.ui.edit_wp", "Edit waypoint", "Изменить вейпоинт"),
    ("megamap.ui.new_wp_title", "New waypoint", "Новый вейпоинт"),
    ("megamap.ui.help_wp", "Waypoint. Right-click: edit, hide, teleport, delete", "Метка. ПКМ: изменить, скрыть, телепорт, удалить"),
    ("megamap.ui.help_list", "Click: show on the map   Double click: edit   Right-click: actions   Wheel: scroll   Type to search", "Клик: показать на карте   Двойной клик: изменить   ПКМ: действия   Колесо: прокрутка   Печатайте для поиска"),
    ("megamap.ui.help_panel", "Click a group's header to fold it   Wheel: scroll   Esc: back", "Клик по заголовку группы сворачивает её   Колесо: прокрутка   Esc: назад"),
    ("megamap.ui.help_preview", "Drag: turn the model   Wheel: zoom", "Тащить: вращать модель   Колесо: приблизить"),
    ("megamap.ui.help", "Drag: pan   Wheel: zoom   Right-click: menu   Toolbar (top right): layers, hover for help   Esc: close", "Тащить: сдвиг   Колесо: масштаб   ПКМ: меню   Панель справа сверху: слои, наведите для подсказки   Esc: закрыть"),
    ("megamap.ui.viewing", "Viewing {0} (you are in {1})", "Просмотр: {0} (вы в {1})"),
    ("megamap.ui.top", "{0}  |  X {1}  Z {2}  |  Explored {3}%  |  1px = {4}", "{0}  |  X {1}  Z {2}  |  Исследовано {3}%  |  1px = {4}"),
    ("megamap.ui.scale_blocks", "{0} blocks", "{0} бл."),
    ("megamap.ui.scale_frac", "1/{0} block", "1/{0} бл."),
    ("megamap.ui.dimension", "Dimension", "Измерение"),
    ("megamap.ui.all", "All", "Все"),
    ("megamap.ui.back", "< Back", "< Назад"),
    ("megamap.ui.you", " (you)", " (вы)"),
    ("megamap.ui.no_dims", "No saved dimensions", "Нет сохранённых измерений"),
    ("megamap.ui.nothing", "Nothing found", "Ничего не найдено"),
    ("megamap.ui.reset_cat", "Reset category", "Сбросить категорию"),
    ("megamap.ui.reset_cat_n", "Reset category ({0})", "Сбросить категорию ({0})"),
    ("megamap.ui.changed", "Changed: {0}", "Изменено: {0}"),
    ("megamap.ui.hover_hint", "Hover a setting for its hint and preview outline", "Наведите на настройку - подсказка и подсветка на превью"),
    ("megamap.ui.minimap", "minimap", "миникарта"),
    ("megamap.ui.map", "map", "карта"),
    ("megamap.ui.clouds_off", "clouds off", "облака выкл."),
    ("megamap.ui.no3d", "no 3D: {0}", "нет 3D: {0}"),
    ("megamap.ui.rendering", "rendering...", "рендер..."),
    ("megamap.ui.layer_badge_surface", "Surface", "Поверхность"),
    ("megamap.ui.layer_badge_cave", "Cave", "Пещера"),
    ("megamap.ui.layer_badge_fixed", "Slice", "Срез"),
    ("megamap.ui.layer_badge_indoor", "Indoors", "В здании"),
    ("megamap.ui.layer_tag_indoor", " · {0}, up to Y {1}", " · {0}, до высоты {1}"),
    ("megamap.ui.layer_auto", " (auto)", " (авто)"),
    ("megamap.layer.indoor", "Indoors", "В здании"),
    ("megamap.ui.layer_tag", " · {0}", " · {0}"),
    ("megamap.ui.layer_tag_cave", " · {0}, Y {1} to {2}", " · {0}, высота {1}–{2}"),
    ("megamap.weather.sunset", "sunset", "закат"),
    ("megamap.weather.night", "night", "ночь"),
    ("megamap.preview.clear", "clear", "ясно"),
    ("megamap.preview.rain", "rain", "дождь"),
    ("megamap.preview.storm", "storm", "гроза"),
    ("megamap.preview.sunset", "sunset", "закат"),
    ("megamap.preview.night", "night", "ночь"),
    ("megamap.preview.nether", "Nether", "Незер"),
    ("megamap.preview.end", "End", "Энд"),
    // Waypoint editing.
    ("megamap.edit.name", "Name: {0}", "Имя: {0}"),
    ("megamap.edit.color", "Colour: {0}", "Цвет: {0}"),
    ("megamap.edit.icon", "Icon: {0}", "Значок: {0}"),
    ("megamap.edit.visible", "Visible: {0}", "Видимый: {0}"),
    ("megamap.edit.save", "Save", "Сохранить"),
    ("megamap.edit.delete", "Delete", "Удалить"),
    ("megamap.edit.share", "Share", "В чат"),
    ("megamap.edit.cancel", "Cancel", "Отмена"),
    ("megamap.color_name.red", "red", "красный"),
    ("megamap.color_name.amber", "amber", "янтарный"),
    ("megamap.color_name.yellow", "yellow", "жёлтый"),
    ("megamap.color_name.green", "green", "зелёный"),
    ("megamap.color_name.cyan", "cyan", "бирюзовый"),
    ("megamap.color_name.blue", "blue", "синий"),
    ("megamap.color_name.purple", "purple", "фиолетовый"),
    ("megamap.color_name.white", "white", "белый"),
    ("megamap.wp_icon.diamond", "diamond", "ромб"),
    ("megamap.wp_icon.star", "star", "звезда"),
    ("megamap.wp_icon.home", "home", "дом"),
    ("megamap.wp_icon.skull", "skull", "череп"),
    ("megamap.wp_icon.flag", "flag", "флаг"),
    ("megamap.wp_icon.cross", "cross", "крест"),
    ("megamap.ent_icon.0", "Square", "Квадрат"),
    ("megamap.ent_icon.1", "Diamond", "Ромб"),
    ("megamap.ent_icon.2", "Triangle", "Треугольник"),
    ("megamap.ent_icon.3", "Cross", "Крест"),
    ("megamap.ent_icon.4", "Ring", "Кольцо"),
    ("megamap.ent_icon.5", "Head", "Голова"),
    ("megamap.ent.mini", "On the minimap: {0}", "На миникарте: {0}"),
    ("megamap.ent.map", "On the map: {0}", "На карте: {0}"),
    ("megamap.ent.color", "Colour: #{0}", "Цвет: #{0}"),
    ("megamap.ent.icon", "Icon: {0}", "Значок: {0}"),
    ("megamap.ent.size", "Size: {0} px", "Размер: {0} px"),
    ("megamap.ent.reset", "Reset to default", "Сбросить"),
    ("megamap.menu.wp_here", "Waypoint here", "Вейпоинт здесь"),
    ("megamap.menu.coords_chat", "Coordinates to chat", "Координаты в чат"),
    ("megamap.menu.share_pos", "Share position", "Поделиться позицией"),
    ("megamap.menu.center", "Center here", "Центрировать здесь"),
    ("megamap.menu.tp", "Teleport here", "Телепорт сюда"),
    ("megamap.menu.wp_edit", "Edit...", "Изменить..."),
    ("megamap.menu.wp_hide", "Hide", "Скрыть"),
    ("megamap.menu.wp_show", "Show", "Показать"),
    ("megamap.menu.wp_center", "Find on the map", "Найти на карте"),
    ("megamap.menu.wp_share", "Send to chat", "Отправить в чат"),
    ("megamap.menu.wp_tp", "Teleport", "Телепорт"),
    ("megamap.menu.wp_delete", "Delete", "Удалить"),
    ("megamap.edit.name_label", "Name", "Имя"),
    ("megamap.edit.color_label", "Colour", "Цвет"),
    ("megamap.edit.icon_label", "Icon", "Значок"),
    ("megamap.edit.visible_label", "Show on the map", "Показывать на карте"),
    ("megamap.tip.zoom_in", "Zoom in (wheel)", "Приблизить (колесо)"),
    ("megamap.tip.zoom_out", "Zoom out (wheel)", "Отдалить (колесо)"),
    ("megamap.tip.center", "Center on player", "К игроку"),
    ("megamap.tip.list", "Waypoints (U)", "Вейпоинты (U)"),
    ("megamap.tip.cave", "Layer: auto / surface / cave / fixed Y", "Слой: авто / поверхность / пещеры / фикс. Y"),
    ("megamap.tip.trail", "Trail", "След"),
    ("megamap.tip.clouds", "Cloud fog over unexplored land", "Облачный туман над неисследованным"),
    ("megamap.tip.relief", "Relief shading: flat / soft / normal / strong", "Тени рельефа: плоско / мягко / обычно / сильно"),
    ("megamap.tip.grid", "Grid: off / chunks / regions / both", "Сетка: выкл / чанки / регионы / оба"),
    ("megamap.tip.settings", "Settings", "Настройки"),
    ("megamap.tip.entities", "Entities on the radar and the map", "Сущности на радаре и карте"),
    ("megamap.tip.dimension", "Show another dimension's map", "Карта другого измерения"),
    // Toasts.
    ("megamap.toast.wp_saved", "Waypoint '{0}' saved", "Вейпоинт «{0}» сохранён"),
    ("megamap.toast.wp_deleted", "Waypoint '{0}' deleted", "Вейпоинт «{0}» удалён"),
    ("megamap.toast.share_failed", "Share failed: {0}", "Не удалось отправить: {0}"),
    ("megamap.toast.layer", "Layer: {0}", "Слой: {0}"),
    ("megamap.toast.clouds_on", "Cloud fog on", "Облачный туман включён"),
    ("megamap.toast.clouds_off", "Cloud fog off", "Облачный туман выключен"),
    ("megamap.toast.relief", "Relief: {0}", "Рельеф: {0}"),
    ("megamap.toast.grid", "Grid: {0}", "Сетка: {0}"),
    ("megamap.toast.reset_cat", "Reset: {0}", "Сброшено: {0}"),
    ("megamap.toast.map_dim", "Map: {0}", "Карта: {0}"),
    ("megamap.toast.death_name", "Death day {0}", "Смерть, день {0}"),
    ("megamap.toast.death", "Death point saved at {0} {1} {2}", "Точка смерти: {0} {1} {2}"),
    ("megamap.toast.wp_default", "Waypoint", "Вейпоинт"),
    ("megamap.toast.wp_added", "Waypoint '{0}' added at {1} {2} {3}", "Вейпоинт «{0}» добавлен: {1} {2} {3}"),
    ("megamap.toast.no_wp", "No waypoint '{0}'", "Нет вейпоинта «{0}»"),
    ("megamap.toast.imported", "Imported '{0}'", "Импортирован «{0}»"),
    ("megamap.toast.no_shared", "No shared waypoint in chat yet", "В чате ещё не было вейпоинтов"),
    ("megamap.toast.no_wp_here", "No waypoints here", "Здесь нет вейпоинтов"),
    ("megamap.toast.trail_cleared", "Trail cleared", "След очищен"),
    ("megamap.toast.usage", "megamap: wp add|del|share <name>, wp list, wp import, trail clear", "megamap: wp add|del|share <имя>, wp list, wp import, trail clear"),
    ("megamap.toast.autozoom", "Auto-zoom: factor {0}x", "Авто-масштаб: множитель {0}x"),
    ("megamap.toast.mini_zoom", "Minimap zoom {0}x", "Масштаб миникарты {0}x"),
    ("megamap.toast.mini_mode", "Minimap: {0}", "Миникарта: {0}"),
    ("megamap.toast.north_up", "Minimap: north up", "Миникарта: север сверху"),
    ("megamap.toast.rotating", "Minimap: rotating", "Миникарта: вращается"),
    ("megamap.toast.wp_shared", "Waypoint '{0}' shared: /megamap wp import", "Вейпоинт «{0}» в чате: /megamap wp import"),
    ("megamap.compass.s", "S", "Ю"),
    ("megamap.compass.sw", "SW", "ЮЗ"),
    ("megamap.compass.w", "W", "З"),
    ("megamap.compass.nw", "NW", "СЗ"),
    ("megamap.compass.n", "N", "С"),
    ("megamap.compass.ne", "NE", "СВ"),
    ("megamap.compass.e", "E", "В"),
    ("megamap.compass.se", "SE", "ЮВ"),
    ("megamap.key.map", "World map", "Карта мира"),
    ("megamap.key.waypoint_here", "Waypoint here", "Вейпоинт здесь"),
    ("megamap.key.waypoints", "Waypoint list", "Список вейпоинтов"),
    ("megamap.key.zoom", "Minimap zoom", "Масштаб миникарты"),
    ("megamap.key.size", "Minimap size", "Размер миникарты"),
    ("megamap.key.mode", "Minimap show / hide", "Показать / скрыть миникарту"),
    ("megamap.key.rotate", "Minimap rotate / lock north", "Вращение миникарты / север сверху"),
    ("megamap.mod.desc", "Minimap, world map and waypoints.", "Миникарта, карта мира и вейпоинты."),
];

/// Interface keys the game itself translates (Minecraft's own words for on, off, back...): with the map in the
/// game's language, those are used so the map reads like the rest of the game.
pub const GAME: &[(&str, &str)] = &[
    ("megamap.on", "options.on"),
    ("megamap.off", "options.off"),
    ("megamap.edit.cancel", "gui.cancel"),
    ("megamap.yes", "gui.yes"),
    ("megamap.no", "gui.no"),
    ("megamap.color_name.red", "color.minecraft.red"),
    ("megamap.color_name.yellow", "color.minecraft.yellow"),
    ("megamap.color_name.green", "color.minecraft.lime"),
    ("megamap.color_name.cyan", "color.minecraft.cyan"),
    ("megamap.color_name.blue", "color.minecraft.blue"),
    ("megamap.color_name.purple", "color.minecraft.purple"),
    ("megamap.color_name.white", "color.minecraft.white"),
    ("megamap.cloud_dim.1", "dimension.minecraft.overworld"),
    ("megamap.cloud_dim.2", "dimension.minecraft.the_nether"),
    ("megamap.cloud_dim.3", "dimension.minecraft.the_end"),
];

/// A game translation lookup: `Some` for a key the game has in its current language.
pub type GameFn = fn(&str) -> Option<String>;

/// The translator the interface draws with: the map's language, and the game's lookup when the game speaks it.
#[derive(Clone, Copy)]
pub struct Tr {
    pub lang: Lang,
    pub game: Option<GameFn>,
}

impl Tr {
    pub const fn new(lang: Lang) -> Tr {
        Tr { lang, game: None }
    }
    /// The text for `key`: the game's active language table first (covering bundled
    /// `assets/megamap/lang/<code>.json` such as `id_id` and resource pack overrides),
    /// then `GAME` shared vanilla keys, then the built-in tables, else the key itself.
    pub fn t(&self, key: &str) -> String {
        if let Some(g) = self.game {
            if let Some(s) = g(key).filter(|s| !s.is_empty()) {
                return s;
            }
            if let Some(&(_, gk)) = GAME.iter().find(|e| e.0 == key) {
                if let Some(s) = g(gk).filter(|s| !s.is_empty()) {
                    return s;
                }
            }
        }
        builtin(self.lang, key).map_or_else(|| key.to_string(), str::to_string)
    }
    /// `t` with `{0}`, `{1}`... replaced by `args`.
    pub fn f(&self, key: &str, args: &[&dyn std::fmt::Display]) -> String {
        fill(&self.t(key), args)
    }
}

/// Replaces `{0}`, `{1}`... in `pattern`.
pub fn fill(pattern: &str, args: &[&dyn std::fmt::Display]) -> String {
    let mut s = pattern.to_string();
    for (i, a) in args.iter().enumerate() {
        s = s.replace(&format!("{{{i}}}"), &a.to_string());
    }
    s
}
