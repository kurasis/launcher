// Русский текст интерфейса (D-038). Same keys and functions as ./en.ts; the
// compiler checks the shape against `Strings`.
import type { OperationKind } from '../contracts';
import type { Strings } from '../strings';

/** 1 файл, 2 файла, 5 файлов. */
export function plural(n: number, one: string, few: string, many: string): string {
  const mod10 = n % 10;
  const mod100 = n % 100;
  if (mod10 === 1 && mod100 !== 11) return one;
  if (mod10 >= 2 && mod10 <= 4 && (mod100 < 12 || mod100 > 14)) return few;
  return many;
}

const files = (n: number) => `${String(n)} ${plural(n, 'файл', 'файла', 'файлов')}`;
const lines = (n: number) => `${String(n)} ${plural(n, 'строка', 'строки', 'строк')}`;

export const ru: Strings = {
  brand: { name: 'Sferum', suffix: 'Launcher' },
  testProfileTag: (name: string) => `Тестовый профиль ${name} · демо-режим`,
  devBuildTag: 'Сборка для разработки',
  independentNotice:
    'Этот лаунчер — независимый проект. Он не связан с Mojang Studios или Microsoft и не одобрен ими.',

  nav: {
    label: 'Главное меню',
    home: 'Установки',
    account: 'Аккаунт',
    settings: 'Настройки',
    diagnostics: 'Диагностика',
    about: 'О программе',
  },

  bootstrap: {
    loading: 'Загрузка состояния лаунчера…',
    errorTitle: 'Не удалось связаться с ядром лаунчера',
    retry: 'Повторить',
    unavailableTitle: 'Данные лаунчера недоступны',
    noResponse: 'Ядро лаунчера не ответило.',
  },

  accountView: {
    title: 'Аккаунт Microsoft',
    signIn: 'Войти через Microsoft',
    signInAgain: 'Войти снова',
    signInHint:
      'Вход открывается в браузере. Лаунчер никогда не видит ваш пароль: он получает от Microsoft только код входа.',
    notConfigured:
      'В этой сборке вход недоступен: не задан собственный идентификатор приложения Microsoft (см. docs/AUTH-SETUP.md).',
    waiting: 'Завершите вход в браузере. Это окно обновится само.',
    cancel: 'Отменить вход',
    checking: 'Проверяем аккаунт…',
    verified: 'Доступ к Minecraft: Java Edition подтверждён для этого сеанса.',
    verifiedNote: 'Доступ проверяется заново при каждом запуске игры.',
    signOut: 'Выйти',
    signOutHint:
      'Выход удаляет сохранённый вход с этого компьютера. Из Microsoft в браузере он вас не выводит.',
    retrySignOut: 'Попробовать выйти ещё раз',
    checkAgain: 'Проверить снова',
    sessionOnly:
      'Системное хранилище паролей недоступно, поэтому после перезапуска лаунчера придётся войти снова.',
    links: {
      buy_minecraft: 'Купить Minecraft',
      create_profile: 'Создать профиль Java',
      xbox_profile: 'Открыть аккаунт Xbox',
      family_settings: 'Семейные настройки Microsoft',
      minecraft_help: 'Справка Minecraft',
      minecraft_eula: 'Лицензия Minecraft (EULA)',
      source_code: 'Исходный код',
    },
    stateTitle: {
      signed_out: 'Вход не выполнен',
      signing_in: 'Выполняется вход',
      refreshing: 'Проверка аккаунта',
      verified_access: 'Вход выполнен',
      no_access: 'Нет доступа к Java Edition',
      profile_setup_required: 'Нужен профиль Java',
      reauth_required: 'Нужно войти снова',
      service_unavailable: 'Служба аккаунтов недоступна',
      app_not_approved: 'Лаунчер не одобрен службами Minecraft',
      restricted: 'Аккаунт ограничен',
      protocol_error: 'Не удалось проверить аккаунт',
      sign_out_pending: 'Выход не завершён',
    },
  },

  account: {
    label: 'Аккаунт',
    state: {
      signed_out: 'Вход не выполнен',
      signing_in: 'Вход…',
      refreshing: 'Проверка аккаунта…',
      verified_access: 'Вход выполнен',
      no_access: 'Нет доступа к Java Edition',
      profile_setup_required: 'Нужно создать профиль',
      reauth_required: 'Нужно войти снова',
      service_unavailable: 'Служба аккаунтов недоступна',
      app_not_approved: 'Лаунчер не одобрен',
      restricted: 'Аккаунт ограничен',
      protocol_error: 'Не удалось проверить аккаунт',
      sign_out_pending: 'Выход не завершён',
    },
  },

  instances: {
    title: 'Установки',
    empty: 'Установок пока нет. Создайте установку, чтобы выбрать версию Minecraft.',
    selected: 'Выбрана',
    create: 'Новая установка',
    health: {
      not_installed: 'Не установлена',
      ready: 'Готова',
      needs_verification: 'Нужна проверка',
      repair_required: 'Нужно восстановление',
      unsupported: 'Не поддерживается',
    },
    busy: 'Занята',
  },

  install: {
    install: 'Установить',
    retry: 'Повторить установку',
    repair: 'Восстановить',
    verify: 'Проверить файлы',
    play: 'Играть',
    playDemo: 'Играть в демо',
    demoHint:
      'Без входа в аккаунт можно играть в демоверсию Minecraft: пять игровых дней, без сетевой игры. Для полной игры войдите в аккаунт, которому принадлежит Minecraft.',
    starting: 'Запуск…',
    cancel: 'Отменить',
    cancelling: 'Отмена…',
    otherBusy: 'Идёт другая операция. Дождитесь её завершения или отмените её.',
    explain: {
      not_installed:
        'Файлы игры, Java и описание версии загружаются с серверов Mojang и Adoptium и проверяются перед использованием.',
      ready:
        'Все файлы проверены. Восстановление проверит каждый файл заново и заменит повреждённые.',
      needs_verification:
        'Файлы не были полностью проверены после последнего изменения. Проверьте их перед игрой.',
      repair_required: 'Некоторые файлы повреждены или отсутствуют. Восстановление заменит их.',
      unsupported: 'Эта установка создана более новой версией лаунчера.',
    },
    userData: 'Восстановление никогда не трогает миры, скриншоты и настройки в папке игры.',
    phase: {
      resolving: 'Чтение описания версии',
      checking_files: 'Проверка файлов',
      downloading: 'Загрузка',
      extracting_runtime: 'Подготовка Java',
      finalizing: 'Завершение',
      cancel_requested: 'Отмена',
    },
    progressLabel: 'Ход загрузки',
    bytes: (done: string, total: string) => `${done} из ${total}`,
    files: (done: number, total: number) => `Проверено файлов: ${String(done)} из ${String(total)}`,
    speed: (rate: string) => `${rate}/с`,
    result: {
      succeeded: (kind: OperationKind, downloaded: number, bytes: string, reused: number) =>
        `${kind === 'repair' ? 'Восстановление' : 'Установка'} завершена. Загружено: ${files(downloaded)} (${bytes}); использовано уже проверенных: ${files(reused)}.`,
      cancelled: 'Отменено. Уже проверенные файлы сохранены; продолжите, чтобы докачать остальное.',
      interrupted: 'Лаунчер остановился до завершения. Уже проверенные файлы сохранены.',
    },
  },

  createForm: {
    title: 'Новая установка',
    name: 'Название',
    version: 'Версия Minecraft',
    versionTypes: {
      release: 'Релизы',
      snapshot: 'Снапшоты',
      old_beta: 'Бета',
      old_alpha: 'Альфа',
    },
    typeHints: {
      release: 'Стабильные версии.',
      snapshot:
        'Тестовые версии перед релизом. Они могут повредить миры — сохраните копии важных миров.',
      old_beta:
        'Бета-версии 2010–2011 годов. Работают на Java 8, демо-режима у них нет, поэтому для игры нужен вход в аккаунт.',
      old_alpha:
        'Версии Alpha, Classic и pre-Classic 2009–2010 годов. Работают на Java 8, демо-режима у них нет, поэтому для игры нужен вход в аккаунт.',
    },
    search: 'Найти версию',
    noMatch: 'Подходящих версий нет.',
    verified: 'Проверена',
    latest: 'Последняя',
    selected: (id: string) => `Выбрана: ${id}`,
    noneSelected: 'Выберите версию.',
    isolation:
      'У каждой установки своя папка игры для миров, настроек и скриншотов. Версию потом сменить нельзя: чтобы играть в другую версию, создайте ещё одну установку.',
    submit: 'Создать установку',
    cancel: 'Отмена',
    availability: {
      available: 'Доступна у Mojang',
      not_in_catalog: 'Нет в каталоге Mojang',
      catalog_unavailable: 'Каталог Mojang недоступен',
      checking: 'Проверяем каталог Mojang…',
    },
    stale: 'Показан последний сохранённый каталог: обновить его не удалось.',
    refresh: 'Проверить снова',
    estimate: 'Оценить загрузку',
    estimating: 'Оцениваем…',
    planTitle: (version: string) => `Оценка загрузки для ${version}`,
    planJava: 'Java',
    planLibraries: 'Библиотеки',
    planAssets: 'Файлы ресурсов',
    planGameFiles: 'Файлы игры',
    planMissing: 'Осталось загрузить',
    planRuntime: 'Размер Java',
    planRuntimeUnknown: 'Станет известен при подготовке Java',
    planNote:
      'Оценка основана только на официальном описании версии. Запуском игры эта версия ещё не проверялась.',
  },

  details: {
    title: 'Настройки установки',
    summaryTitle: 'Сведения',
    version: 'Версия',
    status: 'Состояние',
    openFolder: 'Открыть папку игры',
    openFolderHint: 'Там хранятся миры, скриншоты и настройки этой установки.',
    lastResult: 'Последний результат',
    noResult: 'Пока нет',
    memory: 'Максимум памяти (МиБ)',
    memoryHint: (min: number, max: number, recommended: number) =>
      `От ${String(min)} до ${String(max)} МиБ на этом компьютере. Рекомендуется: ${String(recommended)} МиБ.`,
    lowMemory:
      'На этом компьютере меньше 8 ГиБ памяти. С доступным объёмом игра может работать плохо.',
    width: 'Ширина окна',
    height: 'Высота окна',
    resolutionHint: (minW: number, maxW: number, minH: number, maxH: number) =>
      `Ширина ${String(minW)}–${String(maxW)}, высота ${String(minH)}–${String(maxH)} пикселей.`,
    save: 'Сохранить',
    saved: 'Сохранено.',
    busy: 'Установка занята, её настройки сейчас изменить нельзя.',
    java: 'Java',
    javaNone: 'Выбирается при установке',
    window: 'Память и окно',
    windowValue: (mib: number, width: number, height: number) =>
      `${String(mib)} МиБ · ${String(width)}×${String(height)}`,
    edit: 'Изменить настройки',
  },

  settings: {
    title: 'Настройки',
    sectionsLabel: 'Разделы настроек',
    sections: {
      general: 'Общие',
      java: 'Java',
      defaults: 'Новые установки',
      installations: 'Установки',
    },
    sectionHints: {
      general: 'Язык, оформление, окно лаунчера во время игры и место хранения данных.',
      java: 'Откуда лаунчер загружает Java для новых установок.',
      defaults: 'Память и размер окна для каждой новой установки.',
      installations: 'Память и размер окна существующей установки.',
    },
    javaProvider: 'Источник Java',
    javaProviders: {
      mojang: 'Mojang (рекомендуется)',
      temurin: 'Eclipse Temurin (Adoptium)',
    },
    javaProviderHints: {
      mojang:
        'Сборки Java, которые использует официальный лаунчер Minecraft, — ровно та Java, которую просит каждая версия. Если для этого компьютера у Mojang сборки нет, используется Temurin.',
      temurin:
        'Актуальные сборки Eclipse Temurin той версии Java, которая нужна каждой версии Minecraft.',
    },
    javaApplies:
      'Применяется к новым установкам. Установленная версия сохраняет ту Java, с которой была установлена, а восстановление возвращает именно её.',
    javaInstalled: 'Java каждой установки',
    javaShort: {
      mojang: 'Mojang',
      temurin: 'Temurin',
    },
    javaValue: (provider: string, major: number, release: string) =>
      `${provider} · Java ${String(major)} (${release})`,
    noInstallations: 'Установок пока нет.',
    installation: 'Установка',
    language: 'Язык',
    languages: { en: 'English', ru: 'Русский' },
    theme: 'Тема',
    themes: {
      system: 'Как в системе',
      light: 'Светлая',
      dark: 'Тёмная',
    },
    duringGame: 'Во время игры',
    duringGameModes: {
      unload_and_return: 'Закрывать окно лаунчера и открывать снова после выхода из игры',
      keep_open: 'Оставлять окно лаунчера открытым',
    },
    dataLocation: 'Где хранятся данные лаунчера',
    dataLocationHint:
      'Папку выбирает операционная система для этого пользователя. Изменить её здесь нельзя.',
    defaultsTitle: 'Значения для новых установок',
    defaultsSave: 'Сохранить',
    defaultsSaved: 'Сохранено.',
    readOnly:
      'Настройки доступны только для чтения: они записаны более новой версией лаунчера или не читаются.',
  },

  storage: {
    issuesTitle: 'Замечания о хранилище',
  },

  runs: {
    currentTitle: 'Запущено',
    lastTitle: 'Последний запуск',
    kind: {
      lifecycle_test: 'Тестовый дочерний процесс',
      game: 'Minecraft',
    },
    state: {
      preflight: 'Подготовка',
      spawning: 'Запуск',
      running: 'Работает',
      stopping: 'Остановка',
      exited: 'Завершилась нормально',
      failed: 'Завершилась с ошибкой',
      cancelled: 'Отменено',
      stopped: 'Остановлена вами',
    },
    exitCode: 'Код выхода',
    duration: 'Длительность',
    seconds: (ms: number) => `${(ms / 1000).toFixed(1).replace('.', ',')} с`,
    drainTimedOut:
      'Сбор вывода закрыт через 2 секунды после выхода: другой процесс держал вывод открытым.',
    outputTitle: 'Последние строки вывода',
    dropped: (n: number) => `Более ранние строки не показаны: ${lines(n)}.`,
    stop: 'Остановить',
    stopGame: 'Остановить игру',
    notStarted: 'Не запущено',
    stopWarning:
      'Остановка сразу закрывает Minecraft. Всё несохранённое в текущем мире пропадёт. Чтобы сохраниться, выйдите через меню игры.',
    stopConfirm: 'Остановить без сохранения',
    keepPlaying: 'Продолжить игру',
    loggingDegraded:
      'Журнал вывода не удалось записать на диск; последние строки всё равно показаны здесь.',
  },

  harness: {
    title: 'Тест жизненного цикла (только сборка для разработки)',
    explanation:
      'Запускает безобидную копию этого лаунчера как дочерний процесс, чтобы проверить закрытие и повторное открытие окна. Другие программы она запускать не может и в релизные сборки не входит.',
    scenarios: {
      immediate_exit: 'Выйти сразу',
      short: 'Работать 4 секунды',
      long: 'Работать 12 секунд',
      crash: 'Упасть через 4 секунды',
      inherited_pipe: 'Держать вывод открытым',
      noisy: 'Много вывода',
    },
  },

  quit: {
    button: 'Выход',
    dialogTitle: 'Запуск ещё активен',
    dialogBody:
      'Выход сейчас его остановит. При остановке игры несохранённый прогресс может пропасть. Вместо этого можно оставить лаунчер работать в фоне и открыть его снова, чтобы вернуться.',
    keep: 'Оставить в фоне',
    stop: 'Остановить и выйти',
    cancel: 'Отмена',
    jobTitle: 'Файлы игры ещё готовятся',
    jobBody:
      'Выход сейчас отменит установку или восстановление; уже проверенные файлы сохранятся на следующий раз. Если оставить работу в фоне, окно лаунчера вернётся, когда она закончится.',
    jobStop: 'Отменить и выйти',
  },

  activity: {
    install: 'Установка',
    repair: 'Проверка файлов',
    launch: 'Игра запущена',
    percent: (n: number) => `${String(n)}%`,
  },

  console: {
    title: 'Консоль',
    open: 'Открыть консоль',
    openHint:
      'Всё, что делает лаунчер, и всё, что выводит игра, — в реальном времени, в отдельном окне (Ctrl+Shift+L).',
    sources: { all: 'Всё', launcher: 'Лаунчер', game: 'Игра' },
    levels: { all: 'Все уровни', warn: 'Предупреждения и ошибки', error: 'Только ошибки' },
    search: 'Фильтр строк',
    follow: 'Следить',
    clear: 'Очистить',
    copy: 'Копировать',
    copied: 'Скопировано',
    empty: 'Пока пусто. Установите или запустите игру, чтобы увидеть здесь её вывод.',
    noMatch: 'Ни одна строка не подходит под фильтры.',
    count: (shown: number, total: number) => `Строк: ${String(shown)} из ${String(total)}`,
    missed: (n: number) =>
      `Отброшено старых строк: ${String(n)} (консоль хранит последние 10 000).`,
    unreachable: 'Ядро лаунчера не отвечает. Строки появятся снова, когда оно ответит.',
    sourceTag: { launcher: 'ЛАУНЧЕР', game: 'ИГРА' },
  },

  diagnostics: {
    title: 'Диагностика',
    recentTitle: 'Последние операции',
    columns: {
      operation: 'Операция',
      installation: 'Установка',
      result: 'Результат',
      transferred: 'Загружено',
    },
    none: 'Операций пока не было.',
    kind: { install: 'Установка', repair: 'Восстановление', launch: 'Запуск' },
    outcome: {
      succeeded: 'Успешно',
      failed: 'Ошибка',
      cancelled: 'Отменено',
      interrupted: 'Прервано',
    },
    exportTitle: 'Экспорт диагностики',
    exportExplanation:
      'Создаёт ZIP-файл, который можно приложить к обращению в поддержку. В нём сведения о лаунчере и системе, версии и состояние установок, результаты последних операций и вывод трёх последних запусков, собранный лаунчером. Пути к домашней папке и папке лаунчера сокращаются, токены входа удаляются.',
    exportExcluded:
      'Никогда не включаются: записи аккаунта, токены входа, файлы аргументов запуска, миры, скриншоты, настройки и другие файлы из папок игры.',
    exportButton: 'Экспорт…',
    exported: (name: string) => `Сохранено: ${name}. Проверьте файл перед отправкой.`,
    exportCancelled: 'Экспорт отменён.',
  },

  about: {
    title: 'О программе',
    sourceCode: 'Исходный код',
    license: 'Лицензия: ещё не выбрана. Владелец проекта не опубликовал лицензию на этот код.',
    officialTitle: 'Официальная информация о Minecraft',
    eula: 'Лицензия Minecraft (EULA)',
    help: 'Справка Minecraft',
  },

  foundation: {
    title: 'Состояние проекта',
    body: 'Установка, восстановление и запуск реализованы. Для запуска полной игры нужен вход через Microsoft, который эта сборка не может предложить, пока у проекта нет собственного идентификатора приложения Microsoft.',
  },

  units: {
    bytes: (n: number) => `${String(n)} ${plural(n, 'байт', 'байта', 'байт')}`,
    binary: ['КиБ', 'МиБ', 'ГиБ', 'ТиБ'],
    decimalSeparator: ',',
  },

  systemInfo: {
    title: 'Ядро лаунчера',
    loading: 'Связываемся с ядром лаунчера…',
    errorPrefix: 'Не удалось связаться с ядром лаунчера:',
    retry: 'Повторить',
    version: 'Версия',
    platform: 'Платформа',
    build: 'Сборка',
    framework: 'Tauri',
    buildProfile: {
      debug: 'Отладочная',
      release: 'Релизная',
    },
  },

  microsoftSetup: {
    title: 'Настройка входа через Microsoft',
    status: {
      not_configured: 'Не настроен',
      invalid: 'Неверный идентификатор клиента',
      configured_unverified: 'Настроен, ещё не проверен',
    },
    explanation: {
      not_configured:
        'В этой сборке нет собственного идентификатора приложения Microsoft. Вход останется недоступным, пока владелец проекта его не зарегистрирует. См. docs/AUTH-SETUP.md.',
      invalid:
        'Идентификатор приложения Microsoft, заданный при сборке, — не корректный GUID. Пересоберите с правильным значением. См. docs/AUTH-SETUP.md.',
      configured_unverified:
        'Идентификатор приложения Microsoft задан. В службах Microsoft и Minecraft он ещё не проверялся.',
    },
  },
};
