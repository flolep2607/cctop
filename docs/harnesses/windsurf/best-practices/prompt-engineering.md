# Prompt Engineering

Best practices for crafting effective prompts to get high-quality code from Windsurf, including clear objectives, context, and constraints.

If you're reading this, you're probably someone that already understands some of the use cases and limitations of LLMs. The better prompt and context that provide to the model, the better the outcome will be.

Similarly with Windsurf, there are best practices for crafting more effective prompts to get the most out of the tool, and get the best quality code possible to help you accelerate your workflows.

<Tip>For more complex tasks that may require you to [@-Mention](/chat/overview/#mentions) specific code blocks, use [Chat](/chat/overview) instead of [Command](/command/plugins-overview). </Tip>

## Components of a high quality prompt

* ***Clear objective or outcome***
  * What are you asking the model to produce?
  * Are you asking the model for a plan? For new code? Is it a refactor?
* ***All relevant context to perform the task(s)***
  * Have you properly used @-Mentions to ensure that the proper context is included?
  * Is there any context that is customer specific that may be unclear to Windsurf?
* ***Necessary constraints***
  * Are there any specific frameworks, libraries, or languages that must be utilized?
  * Are there any space or time complexity constraints?
  * Are there any security considerations?

## Examples

***Example #1:***

* **Bad**: Write unit tests for all test cases for an Order Book object.

* **Good**: Using `@class:unit-testing-module` write unit tests for `@func:src-order-book-add` testing for exceptions thrown when above or below stop loss

***Example #2***:

* **Bad**: Refactor rawDataTransform.

* **Good**: Refactor `@func:rawDataTransform` by turning the while loop into a for loop and using the same data structure output as `@func:otherDataTransformer`

***Example #3***:

* **Bad**: Create a new Button for the Contact Form.

* **Good**: Create a new Button component for the `@class:ContactForm` using the style guide in `@repo:frontend-components` that says “Continue”
