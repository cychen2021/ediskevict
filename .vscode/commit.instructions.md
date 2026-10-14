# Tips about the commit messages

- Always use English unless explicitly requested.
- The headline (the first line of a commit message) should be concise and short, LESS THAN 10 WORDS. More contents should be splitted into bullet lists in the body.
- The headline should start with a capitalized verb and end without a period.
- Each bullet point should either be a complete sentence or follow the same grammatical structure as the headline.
- Use `code_item` to refer to code items. Be aware of the backticks around the code item.
- Use `path/filename` (e.g., `.vscode/settings.json`) to refer to a file or directory. Be aware of the backticks around the file path.
- Put important things to the first.
- One line for minor changes.
- One empty line between the headline and the body.
- Avoid general words, including but not limited to "refactor", "enhance" and "improve", unless no other words are more suitable.
- Be aware that the changes may be movement instead of irrelevant deletion and addition when some contents/files are deleted from one file/folder and some contents/files are added to another file/folder. In this case, saying "something is moved from some place to another place" makes much more sense than "something is deleted from/added to somewhere."
- Similarly, saying "something is renamed" is more accurate than "something is deleted and something else is added" when the addition and deletion happens the same place and involve similar items with maybe relative names.
