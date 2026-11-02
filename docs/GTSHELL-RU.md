# Запуск GTShell на Android и ПК

Этот форк PocketHLE запускает оболочку Gizmondo GTShell и подключает отдельную
папку как SD Card. Файлы оболочки, Microsoft DLL и игры берутся из вашей копии.

Исходники: [Katamaran21/PocketHLE-GTShell](https://github.com/Katamaran21/PocketHLE-GTShell).
APK и Windows-версия: [GitHub Releases](https://github.com/Katamaran21/PocketHLE-GTShell/releases).
Скачайте `PocketHLE-0.4.1-android-gtshell-debug.apk` для Android или
`PocketHLE-GTShell-0.4.1-windows-x64.zip` для ПК. Профиль с файлами оболочки и
игрой создаётся отдельно; в публичном релизе этих файлов нет.

## Что нужно

- Android: APK **PocketHLE GTShell**, Android 7.0 или новее, ARM/ARM64 и OpenGL ES 3.
- ПК: Windows 10/11 x64 и **pockethle-gui** из этого форка.
- Полные папки **flash disk** и **flash disk2**. Одного GTShell.exe недостаточно:
  оболочке нужны MFC, PMS DLL, темы, языки и остальные файлы.
- При запуске игры: полная папка SD Card, включая `autorun.exe`, основной EXE,
  каталог `gzga...` и файлы данных.

У проверенной версии GTShell SHA-256:

```text
8E8766071B38AAA0D304FB6105E137F657249E3626B0C57275CF014CFDF01367
```

В предоставленной папке `GizmondoSHELL/flash disk2` правильная версия называлась
`GTShell0.exe`. Генератор профиля распознаёт её и помещает в профиль как
`GTShell.exe`. В рабочей папке `GTShell-PocketHLE-Fork-run/device/flash disk2`
правильный файл уже называется `GTShell.exe`.

## Готовый профиль для вашего Android

В папке `GTShell-PocketHLE-Fork-run` подготовлен
**GTShell-Android-PC-profile.zip**. Он содержит обе папки Flash Disk, правильный
GTShell и SD Card из `Sticky-Balls_Gizmondo_EN`. Профиль переносимый: пути к
карте заданы относительно библиотеки приложения.

1. Скопируйте APK **PocketHLE GTShell** и `GTShell-Android-PC-profile.zip`
   в **Download** телефона.
2. Откройте APK и установите приложение. При запросе Android разрешите установку
   из используемого файлового менеджера. Приложение называется **PocketHLE GTShell**
   и устанавливается отдельно от обычного PocketHLE.
3. Откройте приложение. В библиотеке нажмите **⋮ → Restore library backup (ZIP)**.
4. Выберите `GTShell-Android-PC-profile.zip`. Для первого восстановления выберите
   **Merge (no overwrite)**. В библиотеке появится **GTShell + SD Card**.
5. Нажмите карточку GTShell или **⋮ → Run**. При первом запуске выберите язык
   **English (UK)** и тему **Blue Steel**. Для навигации используйте крестовину,
   для подтверждения — кнопку **Play** справа.
6. Дождитесь инициализации оболочки. Нажмите верхнюю кнопку **Home**, чтобы открыть
   главное меню. В разделе игр выберите запуск с SD Card.
7. Если появляется меню `autorun.exe`, выберите **Sticky Balls** и подтвердите
   кнопкой **Play**. Возврат в GTShell зависит от меню самой игры; кнопка **Exit**
   приложения завершает всю текущую сессию.

Конфигурация в архиве выключает GPRS, реальный GPS, Bluetooth и камеру.
Режим **Merge (no overwrite)** сохраняет существующие глобальные настройки
приложения. При необходимости отключите эти функции в
**Emulator Settings → Gizmondo options**. Для запуска оболочки и локальной
игры SIM-карта не нужна.

Экран Gizmondo имеет размер **320×240**. Настройка **Display and controls → Scale → Auto**
подбирает масштаб для телефона. Фильтр **Nearest** соответствует исходной картинке.
Меню и игровые кнопки приложения остаются с английскими названиями.

## Запуск на вашем ПК

Для уже подготовленной рабочей папки:

1. Откройте `D:\DownloadNEW\GTShell-PocketHLE-Fork-run`.
2. Запустите **run-gtshell-gui-sd-card.cmd**.
3. В GTShell откройте игры и запустите игру с SD Card. Подключается вся папка
   `D:\DownloadNEW\Sticky-Balls_Gizmondo_EN`.

Для оболочки без отдельной SD Card используйте **run-gtshell-gui.cmd**.
На ПК доступны кнопки на изображении Gizmondo и клавиатура; **F10** сохраняет
снимок игровых пикселей. Для завершения сессии используйте возврат в библиотеку.

## Переносимый запуск на другом ПК

1. Распакуйте Windows-версию из GitHub Releases в отдельную папку.
2. Запустите `run-pockethle.cmd`. Библиотека будет храниться в папке `library`
   рядом с эмулятором.
3. Откройте **Game library → Restore library backup (ZIP)…**.
4. Выберите свой профиль ZIP и режим **Merge (no overwrite)**.
5. В библиотеке запустите **GTShell + SD Card**.

При первом восстановлении `Merge (no overwrite)` сохраняет уже существующие
настройки и файлы. Если вы хотите заменить профиль или другую карту, сначала
сохраните свою библиотеку через **Save library backup**, затем выберите подходящий
режим восстановления. **Replace** удаляет содержимое текущей библиотеки.

## Создание профиля из своих папок

Нужен Python 3.9 или новее. Из корня исходников форка выполните одной строкой:

```powershell
python tools/make-gtshell-profile.py --flash-disk "D:/GizmondoSHELL/flash disk" --flash-disk2 "D:/GizmondoSHELL/flash disk2" --sd-card "D:/Sticky-Balls_Gizmondo_EN" --output "D:/GTShell-profile.zip"
```

Замените пути на свои. Для профиля без карты уберите `--sd-card`.
Параметр `--gtshell "путь/к/GTShell.exe"` позволяет явно выбрать другую версию.
Содержимое исходных файлов не изменяется. Используйте физические папки:
Windows junction и символические ссылки генератор отклоняет.

Передавайте генератору **корень SD Card**, где находится `autorun.exe`,
а не только внутреннюю папку `gzga200014`.

После восстановления профиль имеет такую структуру:

```text
library/
  library.json
  config.json
  flash/                         → \Flash Disk\
  games/gtshell/
    game.json
    extracted/                   → \Flash Disk2\
      GTShell.exe
      mfcce400.dll
      ...все файлы оболочки...
    saves/                       → записи в Flash Disk2
  cards/sd-card/                 → \SD Card\ и \Storage Card\
    autorun.exe
    sticky balls.exe
    gzga200014/
```

В `games/gtshell/game.json` и записи GTShell в `library.json` карта задаётся через
`settings.storage_card_dir: "cards/sd-card"`. `max_slices: 0` означает запуск
без ограничения числа срезов CPU; выход выполняется пользователем или гостевой
программой. Сохранения игры лежат в `flash`, настройки оболочки — в `games/gtshell/saves`.

## Если запуск не получается

- **Нет MFC, темы или языка:** восстановите полную папку `flash disk2`, а также
  `flash disk`. Проверьте выбранный GTShell по указанному выше SHA-256.
- **Оболочка работает, но карта не видна:** убедитесь, что `storage_card_dir`
  указывает на существующую папку и что `autorun.exe` лежит в её корне.
- **Профиль ZIP импортировался как игра:** используйте меню **Restore library backup**,
  а не **Import**.
- **Android не устанавливает APK:** проверьте Android 7+, ARM/ARM64 и установку
  именно APK GTShell из этого форка.
- **На ПК отсутствует VCRUNTIME140.dll:** установите
  [Microsoft Visual C++ Redistributable x64](https://aka.ms/vs/17/release/vc_redist.x64.exe).
- **Нужно собрать сведения об ошибке:** включите журнал недостающих API в
  **Emulator Settings**. На ПК файлы находятся в папке библиотеки;
  на Android — в `library/pockethle-unimplemented.log`. Экспорт библиотеки
  через **Save library backup** позволяет получить этот файл без root.

Подробности сборки APK: [ANDROID-BUILD.md](ANDROID-BUILD.md).

Android APK — предварительная сборка, подписанная постоянным локальным debug-ключом.
Запуск GTShell на физическом телефоне ещё не проверен. Windows-пакет содержит
работающее ядро Unicorn и звук; статический видеодекодер в этой Windows-сборке
отключён, поэтому воспроизведение видеовставок не подтверждено.
