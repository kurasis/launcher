// Messages from the Rust core arrive in English (D-038). In another language they
// are translated here: an exact message first, then a known message pattern, then
// a sentence for the error code, and the English text only as a last resort.
// `errors.test.ts` reads the Rust sources and fails when a message has no entry.
import type { ErrorCode, LauncherError } from '../contracts';
import { language } from '../strings';

/** Exact English message → Russian. */
export const ruMessages: Record<string, string> = {
  'A demo player name has 3 to 16 letters, digits or underscores.':
    'Имя игрока в демо — от 3 до 16 латинских букв, цифр или подчёркиваний.',
  'A download location is invalid.': 'Неверный адрес загрузки.',
  'A download source could not be reached. Check the internet connection and try again.':
    'Источник загрузки недоступен. Проверьте подключение к интернету и попробуйте снова.',
  'A download was redirected to a location the launcher does not trust.':
    'Загрузку перенаправили на адрес, которому лаунчер не доверяет.',
  'A file check stopped unexpectedly.': 'Проверка файлов неожиданно остановилась.',
  'A file list is not an archive.': 'Список файлов — не архив.',
  'A game file path contains the Java classpath separator.':
    'Путь к файлу игры содержит разделитель classpath Java.',
  'A game is already running. Only one game can run at a time.':
    'Игра уже запущена. Одновременно может работать только одна игра.',
  'A game path cannot be represented as text for Java.':
    'Путь игры нельзя передать Java в виде текста.',
  'A game path contains characters that Java cannot read from an argument file on this system. Move the launcher data to a folder with a plain-ASCII path.':
    'Путь игры содержит символы, которые Java на этой системе не может прочитать из файла аргументов. Перенесите данные лаунчера в папку с путём только из латинских символов.',
  "A game path contains characters that Java cannot receive with this system's language settings. Use a UTF-8 system locale, or move the launcher data to a folder whose path uses only plain ASCII characters.":
    'Путь игры содержит символы, которые Java не может получить при текущих языковых настройках системы. Включите системную локаль UTF-8 или перенесите данные лаунчера в папку с путём только из латинских символов.',
  'A launch argument is invalid.': 'Неверный аргумент запуска.',
  'A library has the same content as the game client.':
    'Библиотека совпадает по содержимому с клиентом игры.',
  "A metadata document exceeded the launcher's size limit.":
    'Документ с описанием превысил допустимый для лаунчера размер.',
  'A native library archive contains an unsafe entry. Repair the installation.':
    'Архив нативной библиотеки содержит небезопасный элемент. Восстановите установку.',
  'A recorded result could not be read.': 'Не удалось прочитать сохранённый результат.',
  'A runtime check stopped unexpectedly.': 'Проверка Java неожиданно остановилась.',
  'A verified asset index is damaged. Repair the installation.':
    'Проверенный индекс ресурсов повреждён. Восстановите установку.',
  'An encoding check stopped unexpectedly.': 'Проверка кодировки неожиданно остановилась.',
  'An identifier in metadata is not a safe file name.':
    'Идентификатор в описании версии не годится как безопасное имя файла.',
  'Another launcher process is already using the launcher data directory. Close it, or switch to its window.':
    'Папку данных уже использует другой запущенный лаунчер. Закройте его или перейдите в его окно.',
  'Another operation is in progress. Wait for it to finish, or cancel it.':
    'Идёт другая операция. Дождитесь её завершения или отмените её.',
  'Building the diagnostic export failed.': 'Не удалось собрать экспорт диагностики.',
  'Invalid runtime index.': 'Неверный список сборок Java.',
  'Invalid runtime query.': 'Неверный запрос сборки Java.',
  'Java could not be started.': 'Не удалось запустить Java.',
  'Launcher data is unavailable.': 'Данные лаунчера недоступны.',
  'Metadata points to a download location the launcher does not trust.':
    'Описание версии указывает на адрес загрузки, которому лаунчер не доверяет.',
  "Microsoft rejected this launcher's sign-in configuration. The project owner must check the application registration.":
    'Microsoft отклонила настройки входа этого лаунчера. Владельцу проекта нужно проверить регистрацию приложения.',
  'Microsoft sign-in is temporarily unavailable. Try again later.':
    'Вход через Microsoft временно недоступен. Попробуйте позже.',
  "Minecraft services have not approved this launcher's Microsoft application. The project owner must request access (see docs/AUTH-SETUP.md).":
    'Службы Minecraft ещё не одобрили приложение Microsoft этого лаунчера. Владельцу проекта нужно запросить доступ (см. docs/AUTH-SETUP.md).',
  'Minecraft services refused this request. This does not mean the account lacks the game.':
    'Службы Minecraft отклонили запрос. Это не значит, что на аккаунте нет игры.',
  "Mojang's Java runtime list returned data the launcher could not read.":
    'Список сборок Java от Mojang вернул данные, которые лаунчер не смог прочитать.',
  "Mojang's Java runtime manifest did not match its checksum.":
    'Манифест сборки Java от Mojang не совпал с контрольной суммой.',
  'Preparing assets stopped unexpectedly.': 'Подготовка ресурсов неожиданно остановилась.',
  'Preparing native libraries stopped unexpectedly.':
    'Подготовка нативных библиотек неожиданно остановилась.',
  'Saved sign-in credentials could not be removed from the system credential store. Try signing out again.':
    'Не удалось удалить сохранённый вход из системного хранилища паролей. Попробуйте выйти ещё раз.',
  'Serializing a cache document failed.': 'Не удалось записать документ кэша.',
  'Serializing a result failed.': 'Не удалось записать результат.',
  'Settings are read-only because they were written by a newer launcher version or could not be read.':
    'Настройки доступны только для чтения: они записаны более новой версией лаунчера или не читаются.',
  'Sign in again to play.': 'Чтобы играть, войдите снова.',
  'Sign-in was cancelled.': 'Вход отменён.',
  'Sign-in was not completed in time. Start sign-in again.':
    'Вход не завершён вовремя. Начните вход заново.',
  'Some game files are missing or damaged. Repair the installation, then play.':
    'Некоторые файлы игры отсутствуют или повреждены. Восстановите установку, затем играйте.',
  'Storage is not initialized.': 'Хранилище не инициализировано.',
  'The HTTPS client could not be created. Check that the system CA certificates are installed.':
    'Не удалось создать HTTPS-клиент. Проверьте, что в системе установлены корневые сертификаты.',
  'The Java runtime catalog returned data the launcher could not read.':
    'Каталог сборок Java вернул данные, которые лаунчер не смог прочитать.',
  'The Java runtime could not be started for verification.':
    'Не удалось запустить Java для проверки.',
  'The Java runtime package contains unsafe or unsupported content.':
    'Пакет Java содержит небезопасное или неподдерживаемое содержимое.',
  'The Minecraft version catalog could not be read.':
    'Не удалось прочитать каталог версий Minecraft.',
  'The Xbox sign-in returned an inconsistent identity.':
    'Вход Xbox вернул несогласованные данные учётной записи.',
  'The asset index lists the same object with two different sizes.':
    'Индекс ресурсов содержит один и тот же объект с двумя разными размерами.',
  'The catalog cache is missing.': 'Кэш каталога отсутствует.',
  'The file manager could not be opened.': 'Не удалось открыть файловый менеджер.',
  'The game exited with an error. The output below may explain why.':
    'Игра завершилась с ошибкой. Причина может быть видна в выводе ниже.',
  'The installation identifier is invalid.': 'Неверный идентификатор установки.',
  'The installation name cannot contain control characters.':
    'Название установки не может содержать управляющие символы.',
  'The installation no longer exists.': 'Этой установки больше нет.',
  'The installation record does not match its version metadata. Create a new installation for this version.':
    'Запись установки не совпадает с описанием версии. Создайте для этой версии новую установку.',
  'The installation record lists an unsupported checksum.':
    'Запись установки содержит неподдерживаемую контрольную сумму.',
  'The installed Java runtime is not the version or architecture this game needs.':
    'Установленная Java не той версии или архитектуры, которая нужна этой игре.',
  'The launch was cancelled.': 'Запуск отменён.',
  'The launcher data directory must be an absolute path.':
    'Папка данных лаунчера должна задаваться абсолютным путём.',
  'The launcher data directory path cannot be represented as Unicode text, which Java requires for game paths.':
    'Путь к папке данных лаунчера нельзя представить текстом Unicode, а Java требует этого для путей игры.',
  'The launcher data directory path is invalid.': 'Неверный путь к папке данных лаунчера.',
  'The launcher stopped before this operation finished. Verified files were kept; start it again to continue.':
    'Лаунчер остановился до завершения операции. Проверенные файлы сохранены; запустите её снова, чтобы продолжить.',
  'The operating system did not provide a per-user application data directory.':
    'Операционная система не предоставила папку данных приложений для пользователя.',
  'The operation was cancelled.': 'Операция отменена.',
  'The rebuilt Java runtime differs from the recorded one.':
    'Пересобранная Java отличается от записанной.',
  'The request identifier is missing or malformed.':
    'Идентификатор запроса отсутствует или имеет неверный формат.',
  'The request payload is invalid.': 'Неверные данные запроса.',
  'The run disappeared.': 'Запуск пропал.',
  'The run folder path is invalid.': 'Неверный путь к папке запуска.',
  'The save dialog failed.': 'Не удалось открыть окно сохранения.',
  'The sign-in service is rate limiting requests. Wait a moment and try again.':
    'Служба входа ограничивает частоту запросов. Подождите немного и попробуйте снова.',
  'The sign-in service is temporarily unavailable. Try again later.':
    'Служба входа временно недоступна. Попробуйте позже.',
  'The sign-in service refused this request.': 'Служба входа отклонила запрос.',
  'The sign-in service returned a response the launcher does not understand.':
    'Служба входа вернула ответ, который лаунчер не понимает.',
  'The system browser could not be opened.': 'Не удалось открыть системный браузер.',
  'The system credential store is not available. You can stay signed in for this session only.':
    'Системное хранилище паролей недоступно. Вход сохранится только до конца этого сеанса.',
  'The system random number generator failed.': 'Сбой системного генератора случайных чисел.',
  'The test child could not be started.': 'Не удалось запустить тестовый процесс.',
  'The version catalog is unavailable.': 'Каталог версий недоступен.',
  'The version metadata lists the same file with two different sizes.':
    'Описание версии содержит один и тот же файл с двумя разными размерами.',
  'The version metadata uses a launch setting this launcher does not support.':
    'Описание версии использует параметр запуска, который этот лаунчер не поддерживает.',
  'This Microsoft account does not currently have access to Minecraft: Java Edition. You can buy the game or check your subscription on minecraft.net.':
    'У этого аккаунта Microsoft сейчас нет доступа к Minecraft: Java Edition. Купить игру или проверить подписку можно на minecraft.net.',
  'This Minecraft version has no demo mode. Sign in with an account that owns Minecraft to play it.':
    'У этой версии Minecraft нет демо-режима. Чтобы играть в неё, войдите в аккаунт, которому принадлежит Minecraft.',
  "This Minecraft version's metadata uses a format the launcher does not support.":
    'Описание этой версии Minecraft использует формат, который лаунчер не поддерживает.',
  "This Minecraft version's native libraries exist only for Intel Macs. Versions from 1.19 on run on Apple silicon.":
    'Нативные библиотеки этой версии Minecraft есть только для Mac на Intel. На Apple Silicon работают версии начиная с 1.19.',
  'This account has no Minecraft: Java Edition profile yet. Create one at minecraft.net, then try again.':
    'У этого аккаунта ещё нет профиля Minecraft: Java Edition. Создайте его на minecraft.net и попробуйте снова.',
  'This installation is busy. Wait for the current operation to finish.':
    'Установка занята. Дождитесь завершения текущей операции.',
  'This installation is not installed yet. Install it, then play.':
    'Эта установка ещё не установлена. Установите её, затем играйте.',
  'This installation record was written by a newer launcher.':
    'Запись установки создана более новой версией лаунчера.',
  'This installation was created by a newer launcher and is read-only here.':
    'Эта установка создана более новой версией лаунчера и здесь доступна только для чтения.',
  'This installation was created by a newer launcher.':
    'Эта установка создана более новой версией лаунчера.',
  'This is not a valid Minecraft version.': 'Это не версия Minecraft.',
  "This link is not on the launcher's list of allowed destinations.":
    'Этой ссылки нет в списке разрешённых адресов лаунчера.',
  'This operating system or processor is not supported.':
    'Эта операционная система или процессор не поддерживаются.',
  'This operation is not running.': 'Эта операция не выполняется.',
  'This processor architecture is not supported on this operating system.':
    'Эта архитектура процессора не поддерживается в этой операционной системе.',
  'This request ID was already used for another request.':
    'Этот идентификатор запроса уже использован для другого запроса.',
  'This request identifier was already used for a different request.':
    'Этот идентификатор запроса уже использован для другого запроса.',
  'This sign-in was replaced by a newer account action.':
    'Этот вход заменён более новым действием с аккаунтом.',
  'This version is no longer listed by Mojang.': 'Mojang больше не публикует эту версию.',
  "This version's metadata uses a rule the launcher does not support.":
    'Описание этой версии использует правило, которое лаунчер не поддерживает.',
  'Waiting for the game failed.': 'Не удалось дождаться завершения игры.',
  'Waiting for the test child failed.': 'Не удалось дождаться завершения тестового процесса.',
  'Wrong source policy for Adoptium.': 'Неверная политика источника для Adoptium.',
  'Wrong source policy for Mojang.': 'Неверная политика источника для Mojang.',
  'Your sign-in has expired or was revoked. Sign in again.':
    'Срок входа истёк или вход отозван. Войдите снова.',
  'Your sign-in has expired. Sign in again.': 'Срок входа истёк. Войдите снова.',

  // Storage notices (StorageIssue.message).
  'An installation record could not be read.': 'Не удалось прочитать запись установки.',
  'An installation record names a different identifier than its folder and was ignored.':
    'Запись установки указывает идентификатор, отличный от имени её папки, и была пропущена.',
  'An installation record was damaged and was set aside. Its game folder and worlds were not touched.':
    'Запись установки была повреждена и отложена в сторону. Папка игры и миры не тронуты.',
  'An installation summary was corrected in memory but could not be saved.':
    'Сводка установки исправлена в памяти, но сохранить её не удалось.',
  'An interrupted Java runtime installation could not be cleaned up.':
    'Не удалось убрать остатки прерванной установки Java.',
  'An interrupted operation could not be recorded.':
    'Не удалось записать сведения о прерванной операции.',
  'Settings could not be read; defaults are used and changes will not be saved until the launcher is restarted.':
    'Не удалось прочитать настройки; используются значения по умолчанию, и изменения не сохранятся до перезапуска лаунчера.',
  'The compatibility list copy could not be written.':
    'Не удалось записать копию списка совместимости.',
  'The saved account record could not be read.':
    'Не удалось прочитать сохранённую запись аккаунта.',
  'The saved account record was damaged and was set aside. Sign in again.':
    'Сохранённая запись аккаунта была повреждена и отложена в сторону. Войдите снова.',
  'Unfinished download records could not be cleaned up.':
    'Не удалось убрать записи о незавершённых загрузках.',
};

/** Contexts of file errors (`LauncherError::from_io`). */
export const ruIoContexts: Record<string, string> = {
  'Checking the Java runtime failed': 'Не удалось проверить Java',
  'Creating the installation folder': 'Создание папки установки',
  'Creating the launcher data directory': 'Создание папки данных лаунчера',
  'Extracting the Java runtime failed': 'Не удалось распаковать Java',
  'Locating the launcher': 'Поиск лаунчера',
  'Locking the launcher data directory': 'Блокировка папки данных лаунчера',
  'Opening the game folder': 'Открытие папки игры',
  'Preparing the asset index': 'Подготовка индекса ресурсов',
  'Preparing the game files': 'Подготовка файлов игры',
  'Preparing the launch': 'Подготовка запуска',
  'Preparing the sign-in callback': 'Подготовка приёма входа',
  'Reading accounts': 'Чтение аккаунтов',
  'Reading an installation': 'Чтение установки',
  'Reading job records failed': 'Не удалось прочитать записи заданий',
  'Reading settings': 'Чтение настроек',
  'Reading the installation record': 'Чтение записи установки',
  'Reading the job record failed': 'Не удалось прочитать запись задания',
  'Removing incomplete downloads failed': 'Не удалось удалить незавершённые загрузки',
  'Saving Minecraft metadata': 'Сохранение описания Minecraft',
  'Saving account metadata': 'Сохранение данных аккаунта',
  'Saving settings': 'Сохранение настроек',
  'Saving the diagnostic export': 'Сохранение экспорта диагностики',
  'Saving the installation': 'Сохранение установки',
  'Saving the installation record': 'Сохранение записи установки',
  'Saving the job record failed': 'Не удалось сохранить запись задания',
  'Writing a cache file': 'Запись файла кэша',
};

/** What failed to download (`status_error`). */
export const ruDownloads: Record<string, string> = {
  'Minecraft metadata': 'описание Minecraft',
  'the Java runtime catalog': 'каталог сборок Java',
  "Mojang's Java runtime list": 'список сборок Java от Mojang',
  "Mojang's Java runtime manifest": 'манифест сборки Java от Mojang',
};

type Pattern = [RegExp, (...groups: string[]) => string];

const ioContext = (context: string) => ruIoContexts[context] ?? context;

/** Messages with values in them. */
export const ruPatterns: Pattern[] = [
  [
    /^(.+): the disk is full\. Free some space and try again\.$/,
    (c) => `${ioContext(c)}: диск заполнен. Освободите место и попробуйте снова.`,
  ],
  [
    /^(.+): permission denied by the operating system\.$/,
    (c) => `${ioContext(c)}: операционная система запретила доступ.`,
  ],
  [/^(.+): a file operation failed\.$/, (c) => `${ioContext(c)}: файловая операция не удалась.`],
  [
    /^Downloading (.+) failed \(HTTP (\d+)\)\.$/,
    (what, status) => `Не удалось загрузить ${ruDownloads[what] ?? what} (HTTP ${status}).`,
  ],
  [
    /^Downloaded (.+) did not match its expected checksum\.$/,
    (what) => `Загруженный файл (${what}) не совпал с ожидаемой контрольной суммой.`,
  ],
  [
    /^Maximum memory must be between (\d+) and (\d+) MiB on this computer\.$/,
    (min, max) => `Максимум памяти на этом компьютере — от ${min} до ${max} МиБ.`,
  ],
  [
    /^No Java (\d+) runtime is available for this computer from the runtime provider\.$/,
    (major) => `Для этого компьютера нет сборки Java ${major} у этого поставщика.`,
  ],
  [
    /^The installation name must be 1–(\d+) characters\.$/,
    (max) => `Название установки — от 1 до ${max} символов.`,
  ],
  [
    /^The launcher data directory contains the character '(.)', which Java cannot use in a classpath\. Use a directory without it\.$/,
    (c) =>
      `Путь к папке данных лаунчера содержит символ «${c}», который Java не допускает в classpath. Используйте папку без него.`,
  ],
  [
    /^This computer reports (\d+) MiB of memory, which leaves less than (\d+) MiB for the game after the operating system reserve\.$/,
    (total, min) =>
      `Компьютер сообщает о ${total} МиБ памяти; после резерва для системы игре остаётся меньше ${min} МиБ.`,
  ],
  [
    /^Window size must be (\d+)–(\d+) × (\d+)–(\d+) pixels\.$/,
    (a, b, c, d) => `Размер окна: ширина ${a}–${b}, высота ${c}–${d} пикселей.`,
  ],
  [
    /^Settings were damaged and have been reset to defaults\. The damaged file was kept as (.+)\.$/,
    (file) =>
      `Настройки были повреждены и сброшены к значениям по умолчанию. Повреждённый файл сохранён как ${file}.`,
  ],
  [
    /^Settings were damaged and restored from the last good copy\. The damaged file was kept as (.+)\.$/,
    (file) =>
      `Настройки были повреждены и восстановлены из последней исправной копии. Повреждённый файл сохранён как ${file}.`,
  ],
  [
    /^This file was written by a newer launcher version \(schema (\d+)\)\. It is kept unchanged and is read-only here; update the launcher to use it\.$/,
    (schema) =>
      `Этот файл записан более новой версией лаунчера (схема ${schema}). Он сохранён без изменений и здесь доступен только для чтения; обновите лаунчер, чтобы им пользоваться.`,
  ],
];

/** A sentence per error code for messages without their own entry. */
export const ruCodes: Record<ErrorCode, string> = {
  AUTH_CANCELLED: 'Вход отменён.',
  AUTH_TIMEOUT: 'Вход не завершён вовремя. Начните вход заново.',
  AUTH_STATE_MISMATCH: 'Ответ на вход не относится к этой попытке входа. Начните вход заново.',
  AUTH_REAUTH_REQUIRED: 'Нужно войти снова.',
  AUTH_APP_NOT_APPROVED: 'Службы Minecraft ещё не одобрили этот лаунчер.',
  AUTH_ACCOUNT_RESTRICTED: 'Этот аккаунт ограничен (например, семейными настройками Microsoft).',
  AUTH_FORBIDDEN: 'Служба входа отклонила запрос.',
  AUTH_PROTOCOL_ERROR: 'Служба входа вернула ответ, который лаунчер не понимает.',
  AUTH_NOT_CONFIGURED: 'В этой сборке вход недоступен.',
  NO_GAME_ACCESS: 'У этого аккаунта нет доступа к Minecraft: Java Edition.',
  DEMO_UNAVAILABLE: 'У этой версии нет демо-режима.',
  PROFILE_SETUP_REQUIRED: 'У аккаунта ещё нет профиля Minecraft: Java Edition.',
  SERVICE_UNAVAILABLE: 'Служба временно недоступна. Попробуйте позже.',
  SECRET_STORE_UNAVAILABLE: 'Системное хранилище паролей недоступно.',
  SECRET_STORE_DELETE_FAILED: 'Не удалось удалить сохранённый вход из хранилища паролей.',
  UNSUPPORTED_PLATFORM: 'Этот компьютер не поддерживается.',
  UNSUPPORTED_VERSION_METADATA: 'Описание версии использует то, что лаунчер не поддерживает.',
  UNSUPPORTED_SCHEMA: 'Данные записаны более новой версией лаунчера.',
  UNSUPPORTED_PATH_ENCODING: 'Путь содержит символы, которые Java не может принять.',
  UNSUPPORTED_DATA_PATH: 'Папка данных лаунчера не подходит.',
  NETWORK_ERROR: 'Сетевая ошибка. Проверьте подключение к интернету и попробуйте снова.',
  RATE_LIMITED: 'Слишком много запросов. Подождите немного и попробуйте снова.',
  DISK_FULL: 'Диск заполнен. Освободите место и попробуйте снова.',
  PERMISSION_DENIED: 'Операционная система запретила доступ.',
  HASH_MISMATCH: 'Загруженный файл не совпал с ожидаемой контрольной суммой.',
  METADATA_CONFLICT: 'Описание версии противоречиво.',
  UNSAFE_ARCHIVE: 'Пакет содержит небезопасное содержимое.',
  RUNTIME_UNAVAILABLE: 'Подходящая Java недоступна.',
  RUNTIME_MISMATCH: 'Java не подходит для этой игры.',
  INSTANCE_BUSY: 'Установка занята. Дождитесь завершения текущей операции.',
  INSTANCE_NOT_FOUND: 'Этой установки больше нет.',
  GAME_ALREADY_RUNNING: 'Игра уже запущена.',
  GAME_SPAWN_FAILED: 'Не удалось запустить игру.',
  GAME_EXITED_WITH_ERROR: 'Игра завершилась с ошибкой.',
  OPERATION_CANCELLED: 'Операция отменена.',
  OPERATION_INTERRUPTED: 'Операция прервана. Проверенные файлы сохранены.',
  OPERATION_NOT_FOUND: 'Эта операция не выполняется.',
  APP_BUSY: 'Идёт другая операция. Дождитесь её завершения или отмените её.',
  REPAIR_REQUIRED: 'Установку нужно восстановить.',
  REQUEST_ID_CONFLICT: 'Этот идентификатор запроса уже использован.',
  INVALID_INPUT: 'Неверное значение.',
  INSUFFICIENT_MEMORY: 'На этом компьютере недостаточно памяти для игры.',
  DATA_ROOT_LOCKED: 'Папку данных уже использует другой запущенный лаунчер.',
  STORAGE_UNAVAILABLE: 'Данные лаунчера недоступны.',
  STORAGE_ERROR: 'Файловая операция не удалась.',
  INTERNAL: 'Внутренняя ошибка лаунчера.',
};

/** A core message in the current language. */
export function localizeMessage(message: string, code?: ErrorCode): string {
  if (language() !== 'ru') return message;
  const exact = ruMessages[message];
  if (exact !== undefined) return exact;
  for (const [pattern, render] of ruPatterns) {
    const match = pattern.exec(message);
    if (match !== null) return render(...match.slice(1));
  }
  return code !== undefined ? ruCodes[code] : message;
}

export function localizeError(error: Pick<LauncherError, 'code' | 'message'>): string {
  return localizeMessage(error.message, error.code);
}
