<?php
// studio/api/tools.php — что помощник знает о себе и какие действия ему разрешены.
//
// Инструменты выполняет БРАУЗЕР (studio.js): только он видит запущенный
// редактор. Сервер лишь объявляет модели, какие инструменты есть, и решает,
// какие из них доступны в текущем режиме.

/** Инструменты, которые ничего не меняют в документе. Доступны в обоих режимах. */
function studio_read_tools(): array
{
    return [
        [
            'type' => 'function',
            'function' => [
                'name' => 'find_commands',
                'description' => 'Найти команды редактора по смыслу (по-русски или по-английски). '
                    . 'Возвращает id команды, русское название, путь в меню, доступна ли она сейчас и описание параметров. '
                    . 'Вызывай ВСЕГДА перед run_command или open_command: id нельзя угадывать.',
                'parameters' => [
                    'type' => 'object',
                    'properties' => [
                        'query' => ['type' => 'string', 'description' => 'Что нужно сделать, 1–4 слова: «кривые», «удалить фон», «размер изображения», «gaussian blur».'],
                    ],
                    'required' => ['query'],
                ],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'get_state',
                'description' => 'Текущее состояние редактора: открытый документ и его размер, слои, активный слой, выбранный инструмент, открытые диалоги и их поля.',
                'parameters' => ['type' => 'object', 'properties' => new stdClass()],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'look_at_screen',
                'description' => 'Сделать снимок экрана редактора и посмотреть на него. Используй, чтобы проверить результат своих действий или увидеть изображение.',
                'parameters' => ['type' => 'object', 'properties' => new stdClass()],
            ],
        ],
    ];
}

/** Инструменты, которые меняют документ или интерфейс. Только в режиме «Сделай». */
function studio_act_tools(): array
{
    return [
        [
            'type' => 'function',
            'function' => [
                'name' => 'run_command',
                'description' => 'Выполнить команду редактора сразу, без диалога, с заданными параметрами. '
                    . 'Параметры бери из описания params, которое вернул find_commands. Действие попадает в Историю и отменяется.',
                'parameters' => [
                    'type' => 'object',
                    'properties' => [
                        'id' => ['type' => 'string', 'description' => 'id команды из find_commands, например image.adjustments.invert'],
                        'params' => ['type' => 'object', 'description' => 'Параметры команды (JSON-объект). Можно не передавать — тогда значения по умолчанию.'],
                    ],
                    'required' => ['id'],
                ],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'open_command',
                'description' => 'Открыть команду так, как будто пользователь выбрал её в меню: если у команды есть диалог, он откроется, '
                    . 'и пользователь сможет сам подвигать ползунки. Используй, когда пользователь хочет настроить сам или когда параметры неочевидны.',
                'parameters' => [
                    'type' => 'object',
                    'properties' => [
                        'id' => ['type' => 'string', 'description' => 'id команды из find_commands'],
                    ],
                    'required' => ['id'],
                ],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'set_dialog_field',
                'description' => 'Изменить поле в открытом диалоге (самом верхнем). Имена полей видны в get_state → dialogs[].fields.',
                'parameters' => [
                    'type' => 'object',
                    'properties' => [
                        'field' => ['type' => 'string'],
                        'value' => ['description' => 'Новое значение: число, строка, true/false'],
                    ],
                    'required' => ['field', 'value'],
                ],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'confirm_dialog',
                'description' => 'Нажать OK в открытом диалоге (самом верхнем).',
                'parameters' => ['type' => 'object', 'properties' => new stdClass()],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'cancel_dialog',
                'description' => 'Нажать «Отмена» в открытом диалоге (самом верхнем).',
                'parameters' => ['type' => 'object', 'properties' => new stdClass()],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'select_tool',
                'description' => 'Выбрать инструмент на панели инструментов.',
                'parameters' => [
                    'type' => 'object',
                    'properties' => [
                        'tool' => [
                            'type' => 'string',
                            'description' => 'Имя инструмента',
                            'enum' => studio_tool_names(),
                        ],
                    ],
                    'required' => ['tool'],
                ],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'save_as_action',
                'description' => 'Сохранить сделанные шаги как экшен AI-Photoshop (Окно › Экшены), чтобы повторять одной кнопкой или применять к другим фото. '
                    . 'Вызывай, когда пользователь просит «запомни», «сохрани как действие», «сделай так же потом».',
                'parameters' => [
                    'type' => 'object',
                    'properties' => [
                        'name' => ['type' => 'string', 'description' => 'Короткое имя по-русски: «Фото для каталога», «Тёплый портрет»'],
                        'scope' => ['type' => 'string', 'enum' => ['last', 'conversation'], 'description' => 'last — шаги последней работы помощника (по умолчанию); conversation — всё, что сделано за этот разговор'],
                    ],
                    'required' => ['name'],
                ],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'apply_action_to_open_documents',
                'description' => 'Применить сохранённый экшен ко всем открытым документам по очереди (пакетная обработка в браузере).',
                'parameters' => [
                    'type' => 'object',
                    'properties' => [
                        'name' => ['type' => 'string', 'description' => 'Имя экшена'],
                    ],
                    'required' => ['name'],
                ],
            ],
        ],
        [
            'type' => 'function',
            'function' => [
                'name' => 'undo',
                'description' => 'Отменить последние действия (как Ctrl+Z).',
                'parameters' => [
                    'type' => 'object',
                    'properties' => [
                        'steps' => ['type' => 'integer', 'description' => 'Сколько шагов отменить, по умолчанию 1', 'minimum' => 1, 'maximum' => 20],
                    ],
                ],
            ],
        ],
    ];
}

/** Инструменты редактора: имя для select_tool → как он называется по-русски. */
function studio_tools_ru(): array
{
    return [
        'Move' => 'Перемещение (V)', 'RectMarquee' => 'Прямоугольная область (M)', 'EllipseMarquee' => 'Овальная область',
        'Lasso' => 'Лассо (L)', 'PolygonLasso' => 'Прямолинейное лассо', 'MagneticLasso' => 'Магнитное лассо',
        'ObjectSelection' => 'Выделение объектов (W)', 'QuickSelection' => 'Быстрое выделение', 'MagicWand' => 'Волшебная палочка',
        'Crop' => 'Рамка / кадрирование (C)', 'PerspectiveCrop' => 'Кадрирование перспективы', 'Eyedropper' => 'Пипетка (I)',
        'SpotHealing' => 'Точечная восстанавливающая кисть (J)', 'Healing' => 'Восстанавливающая кисть', 'Patch' => 'Заплатка',
        'Remove' => 'Удаление объектов', 'RedEye' => 'Красные глаза', 'Brush' => 'Кисть (B)', 'Pencil' => 'Карандаш',
        'MixerBrush' => 'Микс-кисть', 'CloneStamp' => 'Штамп (S)', 'HistoryBrush' => 'Архивная кисть', 'Eraser' => 'Ластик (E)',
        'BackgroundEraser' => 'Фоновый ластик', 'MagicEraser' => 'Волшебный ластик', 'Gradient' => 'Градиент (G)',
        'PaintBucket' => 'Заливка', 'Blur' => 'Размытие', 'Sharpen' => 'Резкость', 'Smudge' => 'Палец', 'Dodge' => 'Осветлитель (O)',
        'Burn' => 'Затемнитель', 'Sponge' => 'Губка', 'Pen' => 'Перо (P)', 'FreeformPen' => 'Свободное перо', 'CurvaturePen' => 'Перо кривизны',
        'Type' => 'Горизонтальный текст (T)', 'VerticalType' => 'Вертикальный текст', 'PathSelection' => 'Выделение контура (A)',
        'DirectSelection' => 'Стрелка', 'Rectangle' => 'Прямоугольник (U)', 'EllipseShape' => 'Эллипс', 'Triangle' => 'Треугольник',
        'Polygon' => 'Многоугольник', 'Line' => 'Линия', 'CustomShape' => 'Произвольная фигура', 'Hand' => 'Рука (H)',
        'RotateView' => 'Поворот вида (R)', 'Zoom' => 'Масштаб (Z)',
    ];
}

function studio_tool_names(): array
{
    return array_keys(studio_tools_ru());
}

/** Системная инструкция. $mode: 'advice' (Совет) или 'act' (Сделай). */
function studio_system_prompt(string $mode): string
{
    $tools = [];
    foreach (studio_tools_ru() as $id => $ru) {
        $tools[] = "$id — $ru";
    }
    $toolList = implode('; ', $tools);
    $available = $mode === 'act' ? array_merge(studio_read_tools(), studio_act_tools()) : studio_read_tools();
    $catalog = studio_tool_catalog($available);

    $common = <<<TXT
Ты — встроенный помощник графического редактора AI-Photoshop (аналог Adobe Photoshop: те же меню, инструменты и горячие клавиши; интерфейс на русском).
Пользователь работает в редакторе прямо сейчас. К его сообщению приложено состояние программы, а иногда снимок экрана.

Как отвечать:
- Всегда по-русски, коротко и по делу: 1–6 предложений или короткий нумерованный список шагов.
- Пункты меню пиши так: «Изображение › Коррекция › Кривые». Горячие клавиши — в скобках: (Ctrl+M).
- Редактор открыт в браузере: Ctrl+T, Ctrl+N, Ctrl+W, Ctrl+Shift+N браузер забирает себе. Подсказывай замену: свободное трансформирование — F9,
  повторить трансформирование — Shift+F9, новый документ — Ctrl+Alt+N, новый слой — Ctrl+Alt+Shift+N; или режим «Во весь экран» (F11 или кнопка
  в шапке панели помощника) — в нём все сочетания Photoshop работают как обычно.
- Названия команд и путь в меню бери из find_commands, а не по памяти: в AI-Photoshop они могут отличаться от Photoshop.
- Если не уверен, что видишь на снимке, — так и скажи. Не выдумывай функций, которых нет.
- Если документ не открыт, а задача требует документа, подскажи открыть файл (Файл › Открыть, Ctrl+O) или перетащить его в окно.

Инструменты панели (имя для select_tool — русское название): {$toolList}.

Твои инструменты (других нет — ни computer_use, ни кликов по координатам):
{$catalog}
Вызывай их штатно. Если штатный вызов недоступен, напиши вызов текстом, по одному блоку на вызов:
<tool_call><function=ИМЯ><parameter=КЛЮЧ>ЗНАЧЕНИЕ</parameter></function></tool_call>
Значения-объекты и списки пиши в JSON. После блока ничего не пиши — дождись результата.
TXT;

    if ($mode === 'act') {
        return $common . "\n\n" . <<<TXT
Режим «Сделай»: ты сам выполняешь действия в редакторе с помощью инструментов.
Это главное правило: не объясняй пользователю, как сделать вручную, — сделай сам. Если раньше в разговоре
ты был в режиме «Совет» и писал, что не можешь ничего менять, — это больше не так: сейчас режим «Сделай».
Каждое сообщение пользователя начинается с пометки текущего режима — верь ей.
Порядок работы:
1. Пойми задачу. Если она неоднозначна и ошибка дорого стоит — задай один уточняющий вопрос.
2. find_commands — найди нужную команду; проверь, что она enabled. Если disabledReason говорит, что нет документа или выделения, — сначала устрани причину или объясни пользователю.
3. Выполни: run_command с параметрами, когда значения понятны («сделай ярче», «отрази по горизонтали»);
   open_command, когда пользователь хочет настроить сам или видеть диалог.
   Для диалога: open_command → set_dialog_field → confirm_dialog.
4. После изменений изображения посмотри на результат (look_at_screen), если это важно, и коротко скажи, что сделал и как отменить (Ctrl+Z).
Правила безопасности:
- Не закрывай документы, не сохраняй поверх файлов и не удаляй слои, если пользователь прямо об этом не просил.
- Делай не больше 8 действий подряд; если задача больше — сделай часть и спроси, продолжать ли.
Экшены: попросили «запомни это», «сохрани как действие» — save_as_action с понятным именем; «сделай так же со всеми
открытыми фото» — apply_action_to_open_documents. После save_as_action скажи, где его найти (Окно › Экшены).
- Если команда вернула ошибку — не повторяй вслепую: прочитай ошибку, поправь параметры или объясни, что мешает.
TXT;
    }

    return $common . "\n\n" . <<<TXT
Режим «Совет»: ты только объясняешь и подсказываешь, ничего не меняешь сам.
Можно искать команды (find_commands), смотреть состояние (get_state) и экран (look_at_screen).
Объясняй, куда нажать и что выставить, с конкретными числами, если они уместны.
Если пользователь просит сделать что-то за него — скажи, что для этого нужно переключиться в режим «Сделай» (кнопка вверху панели).
TXT;
}

/** Режим «Подсказки к фото»: модель смотрит на снимок и цифры и отвечает строгим JSON. */
function studio_suggest_prompt(): string
{
    return <<<TXT
Ты — помощник фоторедактора AI-Photoshop (как Photoshop). Пользователь только что открыл изображение.
Тебе даны: снимок экрана редактора, точные цифры тонов изображения (document.toneStats) и список команд, которые можно предложить.

Задача: предложи от 0 до 3 самых полезных улучшений именно для этого изображения.
- Опирайся на цифры: low p1/p5 и большой clippedShadowsPct — провалены тени; high p95/p99 и clippedHighlightsPct — пересветы;
  маленький std — мало контраста; низкий saturationMean — блёклые цвета; заметная разница rgbMean — цветовой сдвиг.
- Если изображение уже хорошее, верни пустой список — не придумывай.
- Это не фото (схема, скриншот, рисунок, текст) — пустой список.
- Предпочитай корректирующие слои (layer.newAdjustmentLayer.*): они не портят оригинал.
- Параметры бери строго из описания params команды. Значения — умеренные.
- title — 2–4 слова по-русски, как на кнопке: «Поднять тени», «Добавить контраст», «Убрать синеву».
- why — одно короткое предложение по-русски с опорой на цифры.

Ответ — ТОЛЬКО JSON без пояснений:
{"suggestions":[{"title":"...","why":"...","command":"id из списка","params":{...}}]}
TXT;
}

/**
 * Каталог инструментов текстом — для системной подсказки. Некоторые шлюзы (например, vLLM без
 * парсера вызовов) не передают модели поле tools; тогда модель узнаёт о наших инструментах
 * только отсюда и пишет вызовы текстом.
 */
function studio_tool_catalog(array $tools): string
{
    $lines = [];
    foreach ($tools as $t) {
        $f = $t['function'] ?? [];
        $name = (string)($f['name'] ?? '');
        if ($name === '') {
            continue;
        }
        $params = [];
        foreach (($f['parameters']['properties'] ?? []) as $p => $spec) {
            $params[] = $p . ': ' . ($spec['type'] ?? 'string');
        }
        $lines[] = '- ' . $name . '(' . implode(', ', $params) . ') — ' . (string)($f['description'] ?? '');
    }
    return implode("\n", $lines);
}

/**
 * Вызовы инструментов, которые модель написала текстом, а не штатным полем tool_calls:
 *   <tool_call><function=ИМЯ><parameter=КЛЮЧ>ЗНАЧЕНИЕ</parameter>…</function></tool_call>  (Qwen)
 *   <tool_call>{"name": "ИМЯ", "arguments": {…}}</tool_call>                             (Hermes)
 * Возвращает [вызовы, текст без этих блоков].
 */
function studio_text_tool_calls(string $content): array
{
    $calls = [];
    $value = static function (string $raw) {
        $raw = trim($raw);
        $j = json_decode($raw, true);
        return ($j !== null || $raw === 'null') ? $j : $raw;
    };
    // Qwen XML: с обёрткой <tool_call> или без неё. Прокси с vLLM иногда «съедает» начало
    // вызова («<tool_call>\n<»), и приходит «function=run_command>…» — это тоже понимаем,
    // как и вызов без закрывающих тегов в конце ответа.
    if (preg_match_all('~(?:<tool_call>\s*)?<?function=([A-Za-z0-9_.\-]+)>(.*?)(?:</function>(?:\s*</tool_call>)?|(?=<?function=[A-Za-z0-9_.\-]+>)|\z)~s', $content, $m, PREG_SET_ORDER)) {
        foreach ($m as $one) {
            $args = [];
            if (preg_match_all('~<?parameter=([A-Za-z0-9_.\-]+)>(.*?)(?:</parameter>|(?=<?parameter=[A-Za-z0-9_.\-]+>)|(?=</function>)|\z)~s', $one[2], $pm, PREG_SET_ORDER)) {
                foreach ($pm as $p) {
                    $args[$p[1]] = $value($p[2]);
                }
            }
            $calls[] = ['function' => ['name' => $one[1], 'arguments' => $args ?: new stdClass()]];
            $content = str_replace($one[0], '', $content);
        }
    }
    // Hermes: JSON внутри <tool_call>.
    if (preg_match_all('~<tool_call>\s*(\{.*?\})\s*</tool_call>~s', $content, $m, PREG_SET_ORDER)) {
        foreach ($m as $one) {
            $j = json_decode($one[1], true);
            if (is_array($j) && isset($j['name'])) {
                $args = $j['arguments'] ?? ($j['parameters'] ?? []);
                if (is_string($args)) {
                    $args = json_decode($args, true) ?: [];
                }
                $calls[] = ['function' => ['name' => (string)$j['name'], 'arguments' => $args ?: new stdClass()]];
                $content = str_replace($one[0], '', $content);
            }
        }
    }
    // Рассуждения модели в ответ не нужны.
    $content = preg_replace('~<think>.*?</think>~s', '', $content) ?? $content;
    $content = trim(str_replace(['<tool_call>', '</tool_call>'], '', $content));
    return [$calls, $content];
}

