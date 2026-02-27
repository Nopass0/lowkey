# Lowkey VPN

Полнофункциональный self-hosted VPN-сервис, написанный на Rust с веб-панелью управления, мобильным приложением для Android и десктопным клиентом для Windows/Linux/macOS.

```
┌─────────────────────────────────────────────────────────────────────┐
│                           Lowkey VPN                                 │
├────────────────┬────────────────┬───────────────┬────────────────── │
│  Web Dashboard │ Android Client │ Desktop Client│  CLI Client        │
│  (Next.js)     │  (Kotlin)      │  (Tauri/React)│  (Rust)            │
└───────┬────────┴───────┬────────┴───────┬───────┴───────┬──────────┘
        │ HTTPS          │ HTTPS          │ HTTPS         │ HTTPS
        ▼                ▼                ▼               ▼
┌───────────────────────────────────────────────────────────────────────┐
│                      vpn-server (Rust + Axum)                         │
│  ┌──────────┐  ┌──────────┐  ┌──────────────┐  ┌──────────────────┐  │
│  │ HTTP API │  │ UDP/QUIC │  │  TCP Proxy   │  │   WebSocket      │  │
│  │  :8080   │  │  :51820  │  │  :8388       │  │   fallback       │  │
│  └──────────┘  └──────────┘  └──────────────┘  └──────────────────┘  │
│  ┌──────────────────────────────────────────────────────────────────┐  │
│  │                       PostgreSQL                                  │  │
│  └──────────────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────────────┘
```

## Содержание

- [Возможности](#возможности)
- [Архитектура](#архитектура)
- [Быстрый старт](#быстрый-старт)
- [Конфигурация](#конфигурация)
- [Веб-панель и администрирование](#веб-панель-и-администрирование)
- [Система промокодов](#система-промокодов)
- [Управление релизами и автообновление](#управление-релизами-и-автообновление)
- [REST API](#rest-api)
- [Сборка](#сборка)
- [Деплой в продакшн](#деплой-в-продакшн)
- [Разработка](#разработка)
- [Безопасность](#безопасность)

---

## Возможности

| Функция | Описание |
|---------|----------|
| **Протокол** | UDP tunnel (X25519 + ChaCha20-Poly1305), WebSocket fallback, QUIC/Hysteria2 |
| **Аутентификация** | JWT + bcrypt, реферальная система, промокоды |
| **Платежи** | СБП QR через Точка Банк, ручное пополнение |
| **Промокоды** | 6 типов, комбо-эффекты, индивидуальные коды, гибкие условия |
| **Подписки** | 6 тарифов, пакетные предложения, управление скоростью на пользователя |
| **Реферальная программа** | 25% с платежей рефералов, вывод на карту через СБП |
| **Веб-панель** | Личный кабинет, платежи, промокоды, история, реферальный кабинет |
| **Админ-панель** | Статистика, пользователи, платежи, промокоды, выводы, тарифы, релизы |
| **Android** | Kotlin + Compose, WireGuard-совместимый VPN-сервис, автообновление |
| **Desktop** | Tauri 2 (Windows/Linux/macOS), системный трей, автозапуск, автообновление |
| **CLI клиент** | Полный туннель, сплит-туннель, SOCKS5 прокси |
| **Релизы** | Управление версиями всех платформ из админки, автообновление приложений |
| **Telegram OTP** | Вход в админку через одноразовый код в Telegram |

---

## Архитектура

### Компоненты

```
lowkey/
├── vpn-server/        Rust — HTTP API + UDP/QUIC туннель + TUN + PostgreSQL
├── vpn-client/        Rust — CLI клиент (TUN/SOCKS5/Split-tunnel/QUIC)
├── vpn-common/        Rust — Общая криптография, wire-форматы, Hysteria2
├── web/               Next.js — Веб-панель (личный кабинет + админка)
├── vpn-desktop/       Tauri 2 — Десктопный клиент (React + Rust backend)
├── android-app/       Kotlin + Jetpack Compose — Android клиент
└── migrations/        PostgreSQL схема БД (3 версии)
```

### Транспортные протоколы

| Режим | Протокол | Порт | Применение |
|-------|----------|------|------------|
| Основной | UDP (X25519+ChaCha20) | 51820 | Обычное подключение |
| Fallback | WebSocket | 8080 | Обход блокировок (TCP 80/443) |
| Современный | QUIC/Hysteria2 | 51820 | Высокая скорость, маскировка под HTTPS/3 |
| Прокси | TCP SOCKS5/VLESS | 8388 | Без root, только проксирование трафика |

### Схема шифрования

```
Клиент                                  Сервер
  │                                        │
  │  1. ephemeral X25519 key pair          │
  │  2. POST /api/peers/register           │
  │     { public_key, psk }   ──────────► │  проверка PSK + JWT
  │                           ◄──────────  │  { server_pubkey, vpn_ip, udp_port }
  │                                        │
  │  3. ECDH(client_priv, server_pub)     │
  │     HKDF-SHA256 → symmetric_key       │
  │                                        │
  │  4. UDP packets                        │
  │     [4B: VPN IP][12B: nonce][N+16B]  │
  │     ChaCha20-Poly1305(symmetric_key) ─►│
  │                                        │
```

### Структура базы данных

```
users               promo_codes          app_releases
  id                  id                   id
  login               code                 platform
  password_hash       type                 version (semver)
  balance             value                download_url
  sub_status          extra                file_size_bytes
  sub_expires_at      max_uses             sha256_checksum
  sub_speed_mbps      expires_at           changelog
  vpn_ip              only_new_users       is_latest
  referral_code       target_user_id       released_at
  referral_balance    min_purchase_rub
  first_purchase_done second_type        subscription_plans
  role                second_value         plan_key
                      max_uses_per_user    name
payments                                   price_rub
  id                withdrawal_requests    duration_days
  user_id             id                   speed_mbps
  amount              user_id              is_bundle
  purpose             amount               discount_pct
  plan_id             card_number
  status              bank_name
  qr_url              status
  tochka_order_id     requested_at
```

---

## Быстрый старт

### Требования

- Linux-сервер с root-доступом (Ubuntu 20.04+ / Debian 11+ / CentOS 8+)
- Открытые порты: **8080** (TCP), **51820** (UDP), **8388** (TCP)
- Опционально: PostgreSQL 14+ (скрипт создаст локальную БД автоматически)
- Опционально: Telegram-бот для OTP-входа в админку

### 1. Установка сервера (5 минут)

```bash
git clone https://github.com/Nopass0/lowkey.git
cd lowkey
chmod +x server-setup.sh
sudo ./server-setup.sh
```

Скрипт автоматически:
- Установит системные зависимости (gcc, PostgreSQL, iproute2)
- Установит Rust через rustup
- Создаст базу данных PostgreSQL
- Запросит конфигурацию (или сгенерирует случайные ключи)
- Скомпилирует сервер в release-режиме
- Создаст TUN-интерфейс и настроит iptables NAT
- Запустит сервер в фоне

Управление после установки:
```bash
sudo ./server-setup.sh --status  # проверить статус и подключённых пиров
sudo ./server-setup.sh --run     # перезапустить сервер
sudo ./server-setup.sh --stop    # остановить сервер
sudo ./server-setup.sh --build   # пересобрать без перенастройки
```

### 2. Веб-панель

```bash
# На том же сервере:
cd web
npm install --legacy-peer-deps
NEXT_PUBLIC_API_URL=http://YOUR_SERVER_IP:8080 npm run build
node .next/standalone/server.js
# Веб-панель доступна на http://YOUR_SERVER_IP:3000
```

Или через Docker:
```bash
cp .env.example .env
# Отредактируйте .env
docker compose up -d
```

### 3. Подключение клиента (Linux)

```bash
chmod +x client-setup.sh
sudo ./client-setup.sh
```

Клиент:
- Зарегистрирует аккаунт или войдёт в существующий
- Применит пробный промокод (если указан)
- Запустит VPN-соединение

Повторное подключение:
```bash
sudo ./client-setup.sh --connect   # использует сохранённую конфигурацию
./client-setup.sh --socks5         # SOCKS5 прокси (без root)
sudo ./client-setup.sh --status    # проверить подписку
```

### 4. Android-приложение

1. Соберите APK: `cd android-app && ./gradlew assembleRelease`
2. Или загрузите готовый APK из раздела **Релизы** в веб-панели
3. Введите адрес сервера и создайте аккаунт

### 5. Десктопный клиент (Windows/Linux/macOS)

1. Скачайте установщик со страницы [Downloads](https://ваш-сервер/downloads)
2. Установите и запустите
3. Введите адрес сервера (например: `https://api.lowkeyvpn.com`)
4. Войдите или зарегистрируйтесь

---

## Конфигурация

Скопируйте `.env.example` в `.env` и заполните переменные:

```bash
cp .env.example .env
nano .env
```

### Переменные окружения

| Переменная | Обязательна | Описание |
|-----------|-------------|----------|
| `DATABASE_URL` | ✅ | PostgreSQL connection string |
| `JWT_SECRET` | ✅ | Секрет для подписи JWT-токенов (мин. 32 символа) |
| `VPN_PSK` | ✅ | Pre-shared key для VPN-туннеля |
| `API_PORT` | — | HTTP API порт (по умолчанию: `8080`) |
| `UDP_PORT` | — | UDP VPN-туннель порт (по умолчанию: `51820`) |
| `PROXY_PORT` | — | TCP SOCKS5/VLESS прокси порт (по умолчанию: `8388`) |
| `TG_BOT_TOKEN` | — | Telegram bot token для OTP-входа в админку |
| `TG_ADMIN_CHAT_ID` | — | Ваш Telegram chat ID (для получения кода) |
| `TOCHKA_JWT` | — | JWT-токен API Точка Банк (СБП платежи) |
| `TOCHKA_MERCHANT_ID` | — | Merchant ID Точка Банк |
| `TOCHKA_LEGAL_ID` | — | Legal entity ID Точка Банк |
| `NEXT_PUBLIC_API_URL` | — | URL API для браузера (по умолчанию: `http://localhost:8080`) |

### Аргументы командной строки сервера

```bash
vpn-server --help

Options:
  --api-port <PORT>     HTTP API порт (env: API_PORT, default: 8080)
  --udp-port <PORT>     UDP VPN tunnel порт (env: UDP_PORT, default: 51820)
  --proxy-port <PORT>   TCP proxy порт (env: PROXY_PORT, default: 8388)
  --psk <KEY>           Pre-shared key (env: VPN_PSK)
  --no-nat              Не настраивать iptables NAT (ручная настройка)
  --no-tui              Отключить интерактивный TUI-дашборд
  --public-ip <IP>      Явно указать публичный IP сервера
```

---

## Веб-панель и администрирование

### Личный кабинет пользователя

Доступен по адресу `/dashboard` после входа.

| Раздел | Описание |
|--------|----------|
| **Главная** | Статус подписки, быстрое подключение, кнопка пополнения |
| **Оплата** | Создание СБП QR-платежа, пополнение баланса, покупка тарифа |
| **История** | Список всех платежей и транзакций |
| **Промокоды** | Ввод промокода для получения бонуса |
| **Реферальная программа** | Ссылка для друзей, статистика, заявки на вывод |
| **Настройки** | Изменение пароля |

### Вход в админ-панель

1. Откройте `/admin`
2. Нажмите **Запросить код** — код придёт в ваш Telegram (настройте `TG_BOT_TOKEN` и `TG_ADMIN_CHAT_ID`)
3. Введите 6-значный код

> Если Telegram не настроен, код печатается в логах сервера.

### Tabs администратора

#### Статистика

Ключевые метрики:
- Всего пользователей / активных подписок
- Суммарная выручка
- Незакрытые реферальные выплаты (выделяются оранжевым при ненулевом значении)
- Заморожено под реферальные выплаты

#### Пользователи

Для каждого пользователя:
- ID, логин, роль (user/admin/banned)
- Баланс, статус подписки (с датой истечения)
- Реферальный баланс
- **Лимит скорости** (клик → редактирование прямо в таблице; 0 = безлимит)
- Кнопки Бан / Разбан

#### Платежи

- Полный список платежей (статус, сумма, тариф, дата)
- Кнопка **Подтвердить** для pending-платежей (ручное подтверждение)

#### Промокоды

Создание и управление промокодами. Подробнее — в разделе [Система промокодов](#система-промокодов).

#### Выводы

Управление заявками пользователей на вывод реферального баланса:
- Карточные данные (номер, банк)
- Поле для примечания к платежу
- Кнопки **Одобрить** / **Отклонить**

#### Тарифы

Редактирование цен на тарифы прямо в таблице без перезапуска сервера.

#### Релизы

Управление версиями приложений для всех платформ. Подробнее — в разделе [Управление релизами](#управление-релизами-и-автообновление).

---

## Система промокодов

### Типы промокодов

| Тип | Описание | `value` | `extra` |
|-----|----------|---------|---------|
| `balance` | Пополнение баланса | Сумма (₽) | — |
| `discount` | Скидка на следующий платёж | Процент (%) | — |
| `free_days` | Бесплатные дни подписки | Количество дней | — |
| `speed` | Увеличенная скорость на N дней | Мб/с | Количество дней |
| `subscription` | Активация/продление подписки | Количество дней | Макс. скорость (Мб/с) |
| `combo` | Баланс + дни подписки | Сумма (₽) | Количество дней |

### Дополнительный эффект (`second_type` + `second_value`)

Любой промокод может иметь второй эффект, который применяется параллельно с основным:

```
Пример: промокод типа free_days (7 дней) + second_type=balance (100₽)
→ пользователь получает 7 дней подписки И 100₽ на баланс
```

### Условия применения

| Условие | Поле | Описание |
|---------|------|----------|
| Конкретный пользователь | `target_user_id` | Только указанный user_id может использовать код |
| Только новые | `only_new_users` | Только пользователи без истории покупок |
| Минимальная покупка | `min_purchase_rub` | Баланс или история покупок от N рублей |
| Лимит на юзера | `max_uses_per_user` | Каждый пользователь может применить не более N раз |
| Лимит всего | `max_uses` | Всего применений кода (0 = безлимит) |
| Срок действия | `expires_days` | Код истекает через N дней после создания |

### Примеры создания промокодов

**Одноразовый индивидуальный код для пользователя #42:**
```
Тип: balance
Значение: 500
Макс. использований: 1
Только для user_id: 42
```

**Промо для новых пользователей:**
```
Тип: free_days
Значение: 14
Макс. использований: 0 (∞)
Только новые пользователи: ✓
```

**Новогодняя акция — баланс + бесплатные дни:**
```
Тип: combo
Значение: 200 (рублей)
Extra: 7 (дней)
Описание: Новый год 2025
Срок действия: 31 день
```

**Скидка 20% с минимальной покупкой:**
```
Тип: discount
Значение: 20
Мин. покупка: 299₽
Описание: Зимняя скидка
```

---

## Управление релизами и автообновление

### Загрузка нового релиза

1. Откройте **Админ-панель → Релизы**
2. Заполните форму:
   - **Платформа**: windows / linux / android / macos
   - **Версия**: в формате semver, например `1.2.3`
   - **URL для скачивания**: прямая ссылка на файл
   - **Имя файла**: отображаемое имя (например `LowkeyVPN-1.2.3-setup.exe`)
   - **Размер (байт)**: для отображения на странице загрузок
   - **SHA256**: контрольная сумма файла для верификации
   - **Changelog**: список изменений (поддерживается Markdown)
   - **Сделать текущей**: установить как latest-версию для платформы
3. Нажмите **Добавить релиз**

### Управление существующими релизами

В списке релизов (сгруппированных по платформам):
- **Текущая** (зелёный бейдж) — версия, возвращаемая при авто-проверке обновлений
- Кнопка **Текущая** — сделать релиз актуальным (переключает флаг `is_latest`)
- Кнопка 🗑️ — удалить релиз (soft delete, с подтверждением)

### Публичный API версий

| Метод | Путь | Описание |
|-------|------|----------|
| `GET` | `/api/version/:platform` | Последняя версия для платформы |
| `GET` | `/api/versions` | Последние версии для всех платформ |

Пример ответа:
```json
{
  "id": 5,
  "platform": "android",
  "version": "1.2.3",
  "is_latest": true,
  "download_url": "https://cdn.example.com/lowkey-1.2.3.apk",
  "file_name": "LowkeyVPN-1.2.3.apk",
  "file_size_bytes": 12345678,
  "sha256_checksum": "abc123...",
  "changelog": "- Исправлены баги\n- Улучшена производительность",
  "min_os_version": "8.0",
  "released_at": "2025-01-01T00:00:00Z"
}
```

### Автообновление в приложениях

#### Android
При запуске (если не debug-сборка):
1. Получает текущую версию через `PackageManager`
2. Запрашивает `GET /api/version/android`
3. Если сервер возвращает версию новее — показывает диалог с кнопкой **Скачать**
4. Кнопка открывает `download_url` в браузере

#### Desktop (Tauri)
При запуске (если не debug-сборка):
1. Вызывает команду `check_for_update` в Rust-бэкенде
2. Rust делает запрос к `GET /api/version/windows`
3. Если версия новее текущей — frontend показывает модальное окно с кнопкой **Скачать**
4. Кнопка открывает `download_url` через `@tauri-apps/plugin-shell`

---

## REST API

Базовый URL: `http://YOUR_SERVER:8080`

### Аутентификация

```
Authorization: Bearer <jwt_token>
```

JWT-токен получается при входе/регистрации. Срок действия: 30 дней.

### Auth API

| Метод | Путь | Auth | Описание |
|-------|------|------|----------|
| `POST` | `/auth/register` | — | Регистрация нового пользователя |
| `POST` | `/auth/login` | — | Вход, получение JWT-токена |
| `GET` | `/auth/me` | ✅ | Данные текущего пользователя |

**POST /auth/register**
```json
{
  "login": "user123",
  "password": "password",
  "referral_code": "FRIEND_CODE"  // опционально
}
```

**POST /auth/login**
```json
{ "login": "user123", "password": "password" }
```

Ответ:
```json
{
  "token": "eyJ...",
  "user": {
    "id": 1,
    "login": "user123",
    "balance": 0.0,
    "sub_status": "inactive",
    "sub_expires_at": null,
    "sub_speed_mbps": 0.0,
    "referral_code": "ABC123",
    "referral_balance": 0.0,
    "first_purchase_done": false,
    "role": "user"
  }
}
```

### Subscription API

| Метод | Путь | Auth | Описание |
|-------|------|------|----------|
| `GET` | `/subscription/plans` | — | Список доступных тарифов |
| `POST` | `/subscription/buy` | ✅ | Купить тариф с баланса |

**GET /subscription/plans** — пример ответа:
```json
{
  "plans": [
    {
      "plan_key": "basic",
      "name": "Базовый",
      "price_rub": 199.0,
      "duration_days": 30,
      "speed_mbps": 50.0,
      "is_bundle": false,
      "discount_pct": 0
    }
  ]
}
```

### Payment API

| Метод | Путь | Auth | Описание |
|-------|------|------|----------|
| `POST` | `/payment/sbp/create` | ✅ | Создать СБП QR-платёж |
| `GET` | `/payment/sbp/status/:id` | ✅ | Статус платежа |
| `GET` | `/payment/history` | ✅ | История платежей |

**POST /payment/sbp/create**
```json
{
  "amount": 299.0,
  "purpose": "subscription",  // "balance" | "subscription"
  "plan_id": "standard"       // обязательно если purpose="subscription"
}
```

### Promo API

| Метод | Путь | Auth | Описание |
|-------|------|------|----------|
| `POST` | `/promo/apply` | ✅ | Применить промокод |

```json
{ "code": "PROMO2025" }
```

### Referral API

| Метод | Путь | Auth | Описание |
|-------|------|------|----------|
| `GET` | `/referral/stats` | ✅ | Статистика реферальной программы |
| `POST` | `/referral/withdraw` | ✅ | Запрос на вывод реферального баланса |
| `GET` | `/referral/withdrawals` | ✅ | Список заявок на вывод |

**POST /referral/withdraw**
```json
{
  "amount": 500.0,
  "card_number": "1234567890123456",
  "bank_name": "Сбербанк"
}
```

### VPN API

| Метод | Путь | Auth | Описание |
|-------|------|------|----------|
| `POST` | `/api/peers/register` | ✅ | Зарегистрировать VPN-пир |
| `GET` | `/api/status` | — | Статус сервера |
| `GET` | `/api/peers` | — | Список подключённых пиров |

**POST /api/peers/register**
```json
{}
```
Ответ:
```json
{
  "vpn_ip": "10.66.0.5",
  "server_vpn_ip": "10.66.0.1",
  "udp_port": 51820,
  "psk": "..."
}
```

### Version API (публичный)

| Метод | Путь | Описание |
|-------|------|----------|
| `GET` | `/api/version/:platform` | Последняя версия для платформы |
| `GET` | `/api/versions` | Последние версии всех платформ |

`platform`: `windows`, `linux`, `android`, `macos`

### Admin API

Все admin-эндпоинты требуют заголовок `Authorization: Bearer <admin_token>`.

Admin-токен получается через:
1. `POST /admin/request-code` — отправляет OTP в Telegram
2. `POST /admin/verify-code` с `{ "code": "123456" }` → `{ "token": "..." }`

| Метод | Путь | Описание |
|-------|------|----------|
| `GET` | `/admin/stats` | Сводная статистика |
| `GET` | `/admin/users` | Список пользователей |
| `POST` | `/admin/users/:id/ban` | Забанить/разбанить пользователя |
| `POST` | `/admin/users/:id/limit` | Установить лимит скорости |
| `GET` | `/admin/payments` | Список платежей |
| `POST` | `/admin/payments/:id/confirm` | Подтвердить платёж вручную |
| `GET` | `/admin/promos` | Список промокодов |
| `POST` | `/admin/promos/list` | То же (POST вариант) |
| `POST` | `/admin/promos` | Создать промокод |
| `DELETE` | `/admin/promos/:id` | Удалить промокод |
| `GET` | `/admin/withdrawals` | Список заявок на вывод |
| `POST` | `/admin/withdrawals/:id/approve` | Одобрить вывод |
| `POST` | `/admin/withdrawals/:id/reject` | Отклонить вывод |
| `GET` | `/admin/plans` | Список тарифов |
| `PUT` | `/admin/plans/:key/price` | Изменить цену тарифа |
| `GET` | `/admin/releases` | Список релизов |
| `POST` | `/admin/releases` | Создать релиз |
| `PUT` | `/admin/releases/:id/latest` | Сделать релиз текущим |
| `DELETE` | `/admin/releases/:id` | Удалить релиз |

---

## Сборка

### Требования для разработки

```bash
./dev.sh --setup    # автоустановка всех зависимостей
# или вручную:
# - Rust 1.75+: https://rustup.rs
# - Node.js 18+: https://nodejs.org
# - Android Studio + NDK (только для Android)
# - Tauri 2 prerequisites: https://tauri.app/start/prerequisites/
```

### Сборка всех компонентов

```bash
./build.sh                # собрать всё (Linux)
./build.sh --server       # только Rust-сервер
./build.sh --web          # только Next.js
./build.sh --desktop      # только Tauri-клиент
./build.sh --android      # только Android APK (нужен ANDROID_HOME)
./build.sh --windows      # кросс-компиляция клиента для Windows
./build.sh --clean        # удалить dist/
```

Windows:
```powershell
.\build.ps1               # все компоненты
.\build.ps1 -Component web       # только веб
.\build.ps1 -Component desktop   # только десктоп
.\build.ps1 -Component server    # только Rust
```

### Выходные файлы

```
dist/
├── linux/
│   ├── vpn-server          # Сервер
│   └── vpn-client          # CLI-клиент
├── windows/
│   └── vpn-client.exe      # CLI-клиент для Windows
├── web/                    # Next.js standalone
│   └── server.js           # node server.js для запуска
├── desktop/
│   ├── lowkey-vpn_1.0.0_amd64.deb
│   ├── lowkey-vpn_1.0.0_x86_64.AppImage
│   └── lowkey-vpn_1.0.0_x86_64.rpm
└── android/
    └── LowkeyVPN.apk
```

### Сборка только сервера (ручная)

```bash
# Debug (быстро, для разработки)
cargo build -p vpn-server

# Release (для продакшна)
cargo build --release -p vpn-server

# С логами трассировки
RUST_LOG=debug cargo run -p vpn-server
```

---

## Деплой в продакшн

### Вариант 1: Docker Compose (рекомендуется)

```bash
git clone https://github.com/Nopass0/lowkey.git
cd lowkey

# Настройте переменные окружения
cp .env.example .env
nano .env   # обязательно: JWT_SECRET, VPN_PSK, POSTGRES_PASSWORD

# Запустить
docker compose up -d

# Проверить логи
docker compose logs -f vpn-server
docker compose logs -f web
```

Доступ:
- Веб-панель: http://YOUR_IP:3000
- HTTP API: http://YOUR_IP:8080
- VPN UDP: YOUR_IP:51820

### Вариант 2: systemd-сервис

```bash
sudo ./server-setup.sh   # первичная настройка

# Создать systemd unit
cat > /etc/systemd/system/lowkey-vpn.service << 'EOF'
[Unit]
Description=Lowkey VPN Server
After=network.target postgresql.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/lowkey
EnvironmentFile=/opt/lowkey/.env
ExecStart=/opt/lowkey/target/release/vpn-server --no-tui
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable --now lowkey-vpn
journalctl -u lowkey-vpn -f
```

### Вариант 3: Nginx reverse proxy + SSL

```nginx
# /etc/nginx/sites-available/lowkey
server {
    listen 443 ssl http2;
    server_name api.example.com;

    ssl_certificate     /etc/letsencrypt/live/api.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/api.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
    }
}

server {
    listen 443 ssl http2;
    server_name app.example.com;

    ssl_certificate     /etc/letsencrypt/live/app.example.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/app.example.com/privkey.pem;

    location / {
        proxy_pass http://127.0.0.1:3000;
        proxy_set_header Host $host;
    }
}
```

### Открытие портов в облачном фаерволе

Если используете облачного провайдера, обязательно откройте:

| Порт | Протокол | Назначение |
|------|----------|------------|
| 8080 | TCP | HTTP API (или 443 если за Nginx) |
| 51820 | UDP | VPN-туннель |
| 8388 | TCP | TCP-прокси (SOCKS5/VLESS) |
| 3000 | TCP | Веб-панель (или 443 если за Nginx) |

---

## Разработка

### Быстрый старт для разработчика

```bash
git clone https://github.com/Nopass0/lowkey.git
cd lowkey

# Установить зависимости
./dev.sh --setup

# Настроить БД (потребуется PostgreSQL локально)
cp .env.example .env
nano .env   # DATABASE_URL, JWT_SECRET, VPN_PSK

# Запустить сервер + веб в dev-режиме
./dev.sh              # tmux split-pane (если установлен tmux)
./dev.sh --server     # только сервер
./dev.sh --web        # только веб
./dev.sh --desktop    # только Tauri-клиент
```

### Структура веб-приложения

```
web/
├── app/
│   ├── (auth)/
│   │   ├── login/page.tsx       # Страница входа
│   │   └── register/page.tsx    # Страница регистрации
│   ├── admin/page.tsx           # Админ-панель (все вкладки)
│   ├── dashboard/page.tsx       # Личный кабинет
│   ├── downloads/page.tsx       # Страница загрузок
│   └── layout.tsx               # Корневой layout
├── components/
│   └── LandingPage.tsx          # Главная страница (лендинг)
├── lib/
│   ├── api.ts                   # API-клиент (все эндпоинты)
│   └── utils.ts                 # Утилиты (форматирование и т.д.)
└── store/
    └── auth.ts                  # Zustand: auth + admin tokens
```

### Добавление нового эндпоинта

1. Добавить handler в `vpn-server/src/` (например в `user_api.rs`)
2. Зарегистрировать роут в `vpn-server/src/main.rs`
3. Добавить функцию в `web/lib/api.ts`
4. Использовать в React-компоненте через `useEffect`

### Миграции БД

Миграции запускаются автоматически при старте сервера из директории `migrations/`.

Для добавления новой миграции:
1. Создать файл `migrations/004_description.sql`
2. Добавить `run_migration(pool, include_str!("../../migrations/004_description.sql")).await?;` в `db.rs::run_migrations()`
3. Перезапустить сервер

### Тесты

```bash
cargo test                    # все unit-тесты
cargo test -p vpn-server      # только тесты сервера
cargo test -p vpn-common      # только тесты common-библиотеки

cd web && npm run lint         # линтинг TypeScript
```

---

## Безопасность

### Криптографические примитивы

| Компонент | Алгоритм |
|-----------|----------|
| Обмен ключами | X25519 Diffie-Hellman |
| KDF | HKDF-SHA256 |
| Шифрование туннеля | ChaCha20-Poly1305 (AEAD) |
| Обфускация (Hysteria2) | Salamander XOR (BLAKE2b) |
| Хэширование паролей | Argon2id |
| JWT-подпись | HS256 (HMAC-SHA256) |
| Нонс | 96-bit random (из `OsRng`) |

### Свойства безопасности

- **Нет повторного использования нонсов**: каждый пакет использует свежий случайный 96-bit nonce
- **Прямая секретность клиента**: клиент генерирует ephemeral X25519 ключ на каждое соединение
- **Authenticated encryption**: ChaCha20-Poly1305 обнаруживает любые модификации трафика
- **PSK-защита**: без pre-shared key невозможно зарегистрировать пир
- **JWT-ротация**: токены имеют срок жизни 30 дней
- **Argon2id**: устойчив к GPU/ASIC атакам при брутфорсе паролей

### Рекомендации для продакшна

1. Генерируйте сильные случайные `JWT_SECRET` и `VPN_PSK` (минимум 32 символа)
2. Используйте HTTPS (Nginx + Let's Encrypt) перед API и веб-панелью
3. Ограничьте доступ к `/admin/*` по IP в Nginx при необходимости
4. Регулярно делайте бэкапы PostgreSQL
5. Обновляйте сервер через систему релизов — новые версии содержат патчи безопасности

---

## Лицензия

MIT License — см. [LICENSE](LICENSE)
