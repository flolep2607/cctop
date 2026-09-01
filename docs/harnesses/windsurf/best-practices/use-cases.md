# Common Use Cases

Common use cases for Windsurf including code generation, unit test generation, code documentation, API integration, and code refactoring.

Windsurf serves a variety of use cases at a high level. However, we see certain use cases to be more common than others, especially among our enterprise customers within their production codebases.

## Code generation

<AccordionGroup>
  <Accordion title="Boilerplate code">
    **Guidance:** Windsurf should work well for this use case. Windsurf features including single-line suggestions, multi-line suggestions, and fill-in-the-middle (FIM) completions.

    **Best Practices:** Ensuring usage of Next Completion (`⌥ + ]`), Context Pinning, @ Mentions, and Custom Context will provide best results.
  </Accordion>

  <Accordion title="Front-end development tasks">
    **Guidance:** Windsurf should work well for this use case. Windsurf features including single-line suggestions, multi-line suggestions, and fill-in-the-middle (FIM) completions.

    **Best Practices:** Ensuring usage of Next Completion (`⌥ + ]`), Context Pinning, @ Mentions, and Custom Context will provide best results.
  </Accordion>

  <Accordion title="Back-end development tasks">
    **Guidance:** Windsurf should work well for this use case. Windsurf features including single-line suggestions, multi-line suggestions, and fill-in-the-middle (FIM) completions.

    **Best Practices:** Ensuring usage of Next Completion (`⌥ + ]`), Context Pinning, @ Mentions, and Custom Context will provide best results.
  </Accordion>
</AccordionGroup>

## Unit Test generation

<AccordionGroup>
  <Accordion title="Generate unit tests and automatically remove redundant test cases">
    **Guidance:** Basic usage of Windsurf for generating unit tests should reliably generate 60-70% of unit tests. Edge case coverage will only be as good as the user prompting the model is.

    **Best Practices:** Use @ Mentions. Prompt Engineering best practices. Examples include:

    Write unit test for `@function-name` that tests all edge cases for X and for Y (e.g. email domain).

    Use `@testing-utility-class` to write a unit test for `@function-name`.
  </Accordion>

  <Accordion title="Generate sample data for test execution">
    **Guidance:** Good for low-hanging fruit use cases. For very specific API specs or in-house libraries, Windsurf will not know the intricacies well enough to ensure the quality of generated sample data.

    **Best Practices:** Be very specific about the interface you expect. Think about the complexity of the task (and if a single-shot LLM call will be sufficient to address).
  </Accordion>
</AccordionGroup>

## Internal Code Commentary

<AccordionGroup>
  <Accordion title="Generate in-line comments and code descriptions">
    **Guidance:** Windsurf should work well for this use case. Use Windsurf Command or Windsurf Chat to generate in-line comments and code descriptions.

    **Best Practices:** Use @ Mentions and use Code Lenses as much as possible to ensure the scope of the LLM call is correct.
  </Accordion>

  <Accordion title="Suggest improvements and clarifications">
    **Guidance:** Generally the Refactor button / Windsurf Command would be the best ways to prompt for improvements. Windsurf Chat is the best place to ask for explanations or clarifications. This is a little vague but Windsurf should be good at doing both.

    Windsurf Chat is the best place to ask for explanations or clarifications.

    This is a little vague but Windsurf should be good at doing both.

    **Best Practices**: Use the dropdown prompts (aka Windsurf's Refactor button) - we have custom prompts that are better engineered to deliver the answer you'd more likely expect.
  </Accordion>

  <Accordion title="Automate function headers (C/C++/C#)">
    **Guidance**: The best way to do this would be to create the header file, open chat, @ mention the function in the cpp file, and ask it to write the header function. Then do this iteratively for each in the cpp file. This is the best way to ensure no hallucinations along the way.

    **Best Practices**: Generally avoid trying to write a whole header file with one LLM call. Breaking down the granularity of the work makes the quality of the generated code significantly higher.
  </Accordion>
</AccordionGroup>

## API Documentation and Integration

<AccordionGroup>
  <Accordion title="Create documentation as APIs created and inform proper context">
    **Guidance**: This is similar to test coverage where parts of the API spec that are common across many libraries Windsurf would be able to accurately decorate. However, things that are built special for your in-house use case Windsurf might struggle to do at the quality that you expect.

    **Best Practices**: Similar to test coverage, as much as possible, walk Windsurf's model through the best way to think about what the API is doing and it will be able to decorate better.
  </Accordion>

  <Accordion title="Search repo for APIs with natural language and generate code for integrations">
    **Guidance**: Windsurf's context length for a single LLM call is 16,000 tokens. Thus, depending on the scope of your search, Windsurf's repo-wide search capability may not be sufficient. Repo-wide, multi-step, multi-edit tasks will be supported in upcoming future Windsurf products.

    This is fundamentally a multi-step problem that single-shot LLM calls (i.e. current functionality of all AI code assistants) are not well equipped to address. Additionally, accuracy of result must be much higher than other use cases as integrations are especially fragile.

    **Best Practices**: Windsurf is not well-equipped to solve this problem today. If you'd like to test the extent of Windsurf's existing functionality, build out a step-by-step plan and prompt Windsurf individually with each step and high level of details to guide the AI.
  </Accordion>
</AccordionGroup>

## Code Refactoring

<AccordionGroup>
  <Accordion title="Code simplification and modularization">
    **Guidance**: Ensure proper scoping using Windsurf Code Lenses or @ Mentions to make sure all of the necessary context is passed to the LLM.

    Context lengths for a single LLM call are finite. Thus, depending on the scope of your refactor, this finite context length may be an issue (and for that matter, any single-shot LLM paradigm). Repo-wide, multi-step, multi-edit tasks are now supported in Windsurf's [Cascade](/windsurf/cascade).

    **Best Practices**: Try to break down the prompt as much as possible. The simpler and shorter the command for refactoring the better.
  </Accordion>

  <Accordion title="Restructuring code to improve readability / maintainability">
    **Guidance**: Ensure proper scoping using Windsurf Code Lenses or @ Mentions to make sure all of the necessary context is passed to the LLM.

    Windsurf's context length for a single LLM call is 16,000 tokens. Thus, depending on the scope of your refactor, Windsurf's context length may be an issue (and for that matter, any single-shot LLM paradigm). Repo-wide, multi-step, multi-edit tasks will be supported in upcoming future Windsurf products.

    **Best Practices**: Try to break down the prompt as much as possible. The simpler and shorter the command for refactoring the better.
  </Accordion>
</AccordionGroup>
