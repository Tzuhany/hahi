# Commit Skill

Create a new git commit with a well-crafted message.

## Steps

1. Run `git status` to see all changed files
2. Run `git diff` to understand the changes
3. Analyze the changes and draft a commit message:
   - Summarize the nature of the changes (new feature, bug fix, refactor, etc.)
   - Focus on "why" rather than "what"
   - Keep the first line under 72 characters
4. Stage the relevant files (prefer specific files over `git add -A`)
5. Create the commit

## Guidelines

- Do not commit files that contain secrets (.env, credentials)
- If changes span multiple concerns, suggest splitting into multiple commits
- Use conventional commit format when the project follows it
