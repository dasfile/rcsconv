# Git: как выложить проект на GitHub и как с ним жить

Короткий практикум. Все команды выполняются в каталоге проекта (`C:\dev\rcsconv`
или где он у тебя лежит). Заменяй `<ник>` и `<repo>` своими значениями.

## 1. Один раз настроить

```powershell
git --version                       # git установлен?
git config --global user.name  "Ivan Rodionov"
git config --global user.email "твой@email"     # тот же, что на GitHub
git config --global init.defaultBranch main
```

Если не хочешь публиковать личный e-mail — в GitHub: `Settings → Emails →
Keep my email addresses private`, и укажи в git адрес вида
`<id>+<ник>@users.noreply.github.com`.

В репозитории уже настроено: `.gitignore` (не пускает `golden/`, `out/`, `dumps/`,
`usernotes.txt`), `.gitattributes` (переносы строк и бинарные файлы), ветка `main`.

## 2. Создать репозиторий на GitHub

1. `github.com → New repository`.
2. Имя: `<repo>` (например `rcsconv`), видимость: Public, лицензию/README/.gitignore
   **не добавлять** — они уже есть в проекте.
3. Скопируй адрес вида `https://github.com/<ник>/<repo>.git`.

## 3. Привязать и запушить

```powershell
git remote add origin https://github.com/<ник>/<repo>.git
git remote -v                       # проверить, что origin добавлен
git branch -M main                  # ветка называется main
git push -u origin main
```

**Пароль не подойдёт.** GitHub просит *Personal Access Token*:
`Settings → Developer settings → Personal access tokens → Fine-grained tokens →
Generate new token`, права `Contents: Read and write` на этот репозиторий.
В окне «Password» вставляй токен. Проще один раз поставить
`Git Credential Manager` (идёт с Git for Windows) — тогда токен запомнится.

## 4. Ежедневный цикл

```powershell
git status                 # что изменено (безопасно, ничего не меняет)
git diff                   # что именно изменилось построчно
git add -A                 # подготовить все изменения
git commit -m "что сделано" # зафиксировать снимок
git push                   # отправить на GitHub
```

Полезное:

| Команда | Зачем |
|---|---|
| `git log --oneline -10` | последние 10 коммитов |
| `git diff --stat` | какие файлы изменены и насколько |
| `git show --stat HEAD` | что вошло в последний коммит |
| `git status --short` | короткий вид |

## 5. Если что-то пошло не так

```powershell
git restore <файл>          # отменить несохранённые правки файла
git restore --staged <файл> # убрать файл из подготовки (правки останутся)
git reset --soft HEAD~1     # отменить последний коммит, изменения останутся
git commit --amend -m "…"   # поправить сообщение последнего коммита (до push)
```

«Необратимо» — только `git reset --hard` и `git push --force`; без них всё
восстановимо. Если сомневаешься — сначала `git status` и `git log`.

## 6. Что не коммитить

- `golden/` (данные и эталоны Autodesk), `out/` (результаты), `dumps/`;
- личные заметки (`usernotes.txt` уже в `.gitignore`);
- GitHub не принимает файлы больше 100 МБ — наши данные как раз такие.

Проверка перед push: `git status --short` — в списке не должно быть ничего из
`golden/` или `out/`. Если файл случайно попал в индекс, но не в коммит:
`git restore --staged <файл>`.

## 7. Ветки (когда понадобятся)

```powershell
git switch -c feature/имя     # новая ветка от текущей
git switch main               # вернуться
git merge feature/имя         # влить в main (из main)
```

## 8. Что уже лежит в репозитории для GitHub

- `README.md` — витрина (что умеет, как проверялось, как это сделано);
- `LICENSE` (MIT), `pyproject.toml` (метадата), `.gitattributes`;
- `.github/workflows/tests.yml` — после первого push GitHub сам прогонит 85 тестов
  на Python 3.11/3.12 и покажет галочку;
- бейджи в README (добавить после создания репозитория):
  `![tests](https://github.com/<ник>/<repo>/actions/workflows/tests.yml/badge.svg)`
  и `![license](https://img.shields.io/badge/license-MIT-blue)`.
