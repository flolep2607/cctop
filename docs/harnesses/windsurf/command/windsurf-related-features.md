# Code Lenses

Use Windsurf code lenses for quick Explain, Refactor, and Docstring operations on functions and classes directly in the editor.

## Explain, Refactor, and Add Docstring

At the top of the text editor, Windsurf gives exposes *code lenses* on functions and classes.

<Frame>
  <img />
</Frame>

The `Explain` code lens will invoke Cascade, which will simply explain what the function or class does and how it works.

The `Refactor` and `Docstring` code lenses in particular will invoke Command.

* If you click `Refactor`, Windsurf will prompt you with a dropdown of selectable, pre-populated
  instructions that you can choose from. You can also write your own. This is equivalent to highlighting the function and invoking Command.
* If you click `Docstring`, Windsurf will generate a docstring for you above the function header.
  (In Python, the docstring will be correctly generated *underneath* the function header.)

<Frame>
  <video />
</Frame>
