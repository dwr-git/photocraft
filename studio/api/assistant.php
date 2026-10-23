<?php
// studio/api/assistant.php — помощник PhotoCraft: мост между панелью в браузере и Ollama.
//
//   GET  assistant.php            проверка: вошёл ли пользователь, какая модель, доступна ли она
//   POST assistant.php            {mode: "advice"|"act", messages: [...]} → {ok, message: {content, tool_calls}}
//
// Инструменты (find_commands, run_command…) выполняет браузер: только он видит
// запущенный редактор. Здесь — авторизация, системная инструкция, список
// разрешённых инструментов и запрос к модели.

declare(strict_types=1);

header('Content-Type: application/json; charset=utf-8');
header('Cache-Control: no-store, max-age=0');
header('X-Content-Type-Options: nosniff');

/** Ответ с ошибкой и выход. */
function studio_fail(int $http, string $message, array $extra = []): void
{
    http_response_code($http);
    echo json_encode(['ok' => false, 'error' => $message] + $extra, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    exit;
}

// Предупреждения PHP не должны ломать JSON-ответ.
ini_set('display_errors', '0');
set_exception_handler(static function (Throwable $e): void {
    error_log('photocraft assistant: ' . $e->getMessage());
    studio_fail(500, 'Внутренняя ошибка помощника. Подробности — в журнале PHP.');
});

$STUDIO = require __DIR__ . '/settings.php';
require_once __DIR__ . '/tools.php';

// ---------------------------------------------------------------- авторизация
// auth.php системы подключает config.php: после него известны $url_ollama,
// $global_model, enrichLLMPayload() и setLLMApp().
$__authed = false;
if ($STUDIO['auth_file'] !== '' && is_file($STUDIO['auth_file'])) {
    require_once $STUDIO['auth_file'];
    if (!function_exists('isLoggedIn') || !isLoggedIn()) {
        studio_fail(401, 'Войдите в систему, чтобы пользоваться помощником.', ['login' => true]);
    }
    $__authed = true;
} elseif (!$STUDIO['allow_anonymous']) {
    studio_fail(500, 'Не найден auth.php системы. Укажите путь в PHOTOCRAFT_AUTH_FILE (см. studio/README.ru.md).');
}
// Сессия больше не нужна: отпускаем её блокировку, иначе пока модель думает,
// остальные страницы этого пользователя будут ждать.
if (session_status() === PHP_SESSION_ACTIVE) {
    session_write_close();
}
if (function_exists('setLLMApp')) {
    setLLMApp('photocraft');
}

// ---------------------------------------------------------------- модель
$ollamaUrl = studio_env('PHOTOCRAFT_OLLAMA_URL');
if ($ollamaUrl === '') {
    $hostPort = (isset($url_ollama) && is_string($url_ollama) && trim($url_ollama, ':') !== '') ? $url_ollama : '';
    if ($hostPort === '') {
        $ip = studio_env('OLLAMA_IP', '127.0.0.1');
        $port = studio_env('OLLAMA_PORT', '11434');
        $hostPort = "$ip:$port";
    }
    $ollamaUrl = preg_match('#^https?://#i', $hostPort) ? $hostPort : 'http://' . $hostPort;
}
$ollamaUrl = rtrim($ollamaUrl, '/');
$model = studio_env('PHOTOCRAFT_MODEL') ?: ((isset($global_model) && is_string($global_model) && $global_model !== '') ? $global_model : studio_env('GLOBAL_MODEL', 'ministral-3:8b'));
$ctx = (int)(studio_env('PHOTOCRAFT_CONTEXT') ?: ((isset($ollama_ctxsize) && (int)$ollama_ctxsize > 0) ? $ollama_ctxsize : '32768'));

// ---------------------------------------------------------------- GET: проверка
if (($_SERVER['REQUEST_METHOD'] ?? 'GET') === 'GET') {
    echo json_encode([
        'ok' => true,
        'model' => $model,
        'authenticated' => $__authed,
        'user' => ($__authed && function_exists('getCurrentUser')) ? (getCurrentUser()['login'] ?? null) : null,
    ], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
    exit;
}
if (($_SERVER['REQUEST_METHOD'] ?? '') !== 'POST') {
    studio_fail(405, 'Метод не поддерживается.');
}

// ---------------------------------------------------------------- запрос
$len = (int)($_SERVER['CONTENT_LENGTH'] ?? 0);
if ($len > $STUDIO['max_body_bytes']) {
    studio_fail(413, 'Слишком большой запрос (скриншот?). Попробуйте ещё раз — панель уменьшит снимок.');
}
$raw = file_get_contents('php://input', false, null, 0, $STUDIO['max_body_bytes'] + 1);
if ($raw === false || strlen($raw) > $STUDIO['max_body_bytes']) {
    studio_fail(413, 'Слишком большой запрос.');
}
$in = json_decode($raw, true, 64);
if (!is_array($in) || !isset($in['messages']) || !is_array($in['messages'])) {
    studio_fail(400, 'Неверный формат запроса: нужен JSON {mode, messages}.');
}
$mode = in_array($in['mode'] ?? '', ['act', 'suggest'], true) ? $in['mode'] : 'advice';

/** Оставляет в сообщениях только то, что понимает Ollama, и проверяет размеры. */
function studio_clean_messages(array $messages, array $limits): array
{
    if (count($messages) === 0) {
        studio_fail(400, 'Пустой диалог.');
    }
    // Старые сообщения отрезаем, системная инструкция добавляется сервером.
    $messages = array_slice(array_values($messages), -$limits['max_messages']);
    $out = [];
    $images = 0;
    foreach ($messages as $m) {
        if (!is_array($m)) {
            continue;
        }
        $role = (string)($m['role'] ?? '');
        if (!in_array($role, ['user', 'assistant', 'tool'], true)) {
            continue;
        }
        $content = $m['content'] ?? '';
        if (!is_string($content)) {
            $content = json_encode($content, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES) ?: '';
        }
        $msg = ['role' => $role, 'content' => mb_substr($content, 0, 20000)];
        if ($role === 'user' && !empty($m['images']) && is_array($m['images'])) {
            $imgs = [];
            foreach ($m['images'] as $img) {
                if (!is_string($img)) {
                    continue;
                }
                $img = preg_replace('#^data:image/[a-z+]+;base64,#i', '', $img);
                if (strlen($img) > $limits['max_image_bytes'] || !preg_match('#^[A-Za-z0-9+/]+={0,2}$#', $img)) {
                    continue;
                }
                $imgs[] = $img;
            }
            // Картинки держим только в последних сообщениях: каждая занимает много контекста.
            if ($imgs) {
                $msg['images'] = $imgs;
                $images += count($imgs);
            }
        }
        if ($role === 'assistant' && !empty($m['tool_calls']) && is_array($m['tool_calls'])) {
            $calls = [];
            foreach (array_slice($m['tool_calls'], 0, 16) as $c) {
                $fn = $c['function'] ?? null;
                if (!is_array($fn) || !isset($fn['name']) || !is_string($fn['name'])) {
                    continue;
                }
                $args = $fn['arguments'] ?? [];
                if (is_string($args)) {
                    $decoded = json_decode($args, true);
                    $args = is_array($decoded) ? $decoded : [];
                }
                $calls[] = ['function' => ['name' => $fn['name'], 'arguments' => is_array($args) && $args ? $args : new stdClass()]];
            }
            if ($calls) {
                $msg['tool_calls'] = $calls;
            }
        }
        if ($role === 'tool') {
            $name = (string)($m['tool_name'] ?? $m['name'] ?? '');
            if ($name !== '') {
                $msg['tool_name'] = $name;
            }
        }
        $out[] = $msg;
    }
    // Не больше max_images картинок на запрос: оставляем самые свежие.
    if ($images > $limits['max_images']) {
        $keep = $limits['max_images'];
        for ($i = count($out) - 1; $i >= 0; $i--) {
            if (!isset($out[$i]['images'])) {
                continue;
            }
            if ($keep <= 0) {
                unset($out[$i]['images']);
                continue;
            }
            $out[$i]['images'] = array_slice($out[$i]['images'], -$keep);
            $keep -= count($out[$i]['images']);
        }
    }
    return $out;
}

$messages = studio_clean_messages($in['messages'], $STUDIO);
$tools = studio_read_tools();
if ($mode === 'act') {
    $tools = array_merge($tools, studio_act_tools());
}
$system = $mode === 'suggest' ? studio_suggest_prompt() : studio_system_prompt($mode);

$payload = [
    'model' => $model,
    'messages' => array_merge([['role' => 'system', 'content' => $system]], $messages),
    'stream' => false,
    'think' => false,
    'options' => [
        'temperature' => 0.2,
        'num_predict' => max(256, $STUDIO['max_tokens']),
        'num_ctx' => max(4096, $ctx),
    ],
];
if ($mode === 'suggest') {
    // Подсказки — один строгий JSON, без инструментов.
    $payload['format'] = 'json';
} else {
    $payload['tools'] = $tools;
}
if (function_exists('enrichLLMPayload')) {
    enrichLLMPayload($payload, 'photocraft'); // трекинг на шлюзе, как у остальных приложений
}

// ---------------------------------------------------------------- запрос к модели
$ch = curl_init($ollamaUrl . '/api/chat');
curl_setopt_array($ch, [
    CURLOPT_RETURNTRANSFER => true,
    CURLOPT_POST => true,
    CURLOPT_HTTPHEADER => ['Content-Type: application/json'],
    CURLOPT_POSTFIELDS => json_encode($payload, JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES),
    CURLOPT_CONNECTTIMEOUT => 10,
    CURLOPT_TIMEOUT => max(30, $STUDIO['timeout']),
]);
$started = microtime(true);
$body = curl_exec($ch);
$curlError = curl_error($ch);
$http = (int)curl_getinfo($ch, CURLINFO_HTTP_CODE);
curl_close($ch);

if ($body === false || $curlError !== '') {
    studio_fail(502, 'Модель недоступна: ' . $curlError);
}
$res = json_decode((string)$body, true);
if (!is_array($res)) {
    studio_fail(502, 'Модель вернула непонятный ответ (HTTP ' . $http . ').');
}
// Отказ шлюза или Ollama: {"error": "..."} или {"error": {"message": "..."}}
if (isset($res['error'])) {
    $err = is_array($res['error']) ? (string)($res['error']['message'] ?? json_encode($res['error'], JSON_UNESCAPED_UNICODE)) : (string)$res['error'];
    // Модель не умеет вызывать инструменты — скажем об этом понятно.
    if (stripos($err, 'does not support tools') !== false) {
        studio_fail(400, "Модель «{$model}» не умеет вызывать инструменты. Укажите в .env PHOTOCRAFT_MODEL с поддержкой tools (например ministral-3:8b или qwen3).");
    }
    if (stripos($err, 'image') !== false && stripos($err, 'support') !== false) {
        studio_fail(400, "Модель «{$model}» не принимает картинки. Выключите «глаз» в панели или укажите мультимодальную модель в PHOTOCRAFT_MODEL.", ['noVision' => true]);
    }
    studio_fail($http >= 400 ? $http : 502, '⚠️ ' . $err);
}

$msg = $res['message'] ?? [];
$calls = [];
foreach (($msg['tool_calls'] ?? []) as $c) {
    $fn = $c['function'] ?? null;
    if (!is_array($fn) || !isset($fn['name'])) {
        continue;
    }
    $args = $fn['arguments'] ?? [];
    if (is_string($args)) {
        $decoded = json_decode($args, true);
        $args = is_array($decoded) ? $decoded : [];
    }
    $calls[] = ['function' => ['name' => (string)$fn['name'], 'arguments' => $args ?: new stdClass()]];
}

echo json_encode([
    'ok' => true,
    'model' => $res['model'] ?? $model,
    'message' => [
        'role' => 'assistant',
        'content' => (string)($msg['content'] ?? ''),
        'tool_calls' => $calls,
    ],
    'finish' => (string)($res['done_reason'] ?? ''),
    'usage' => [
        'prompt_tokens' => (int)($res['prompt_eval_count'] ?? 0),
        'completion_tokens' => (int)($res['eval_count'] ?? 0),
    ],
    'seconds' => round(microtime(true) - $started, 2),
], JSON_UNESCAPED_UNICODE | JSON_UNESCAPED_SLASHES);
