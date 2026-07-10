# SpaceWasm Generative AI Usage Guidelines

## Our Position on Generative AI

The primary purpose of SpaceWasm is to provide a safe boundary between untrusted or low-trust executables a
path to execute on-board in a way that cannot pollute the rest of the system. To this end many design
decisions require maticulous consideration.

Our stance on Generative AI contributions for SpaceWasm is to generally **disallow** its use for development of the interpreter (code found in `src/*`).

We have the following exceptions to this rule:

1. Diagnosis and reporting of bugs or security vulnerabilities
2. Use for integration or unit tests.
3. Utilities and tooling (`fuzz/*`, `crates/*`)
4. Repository CI (`.github/*`)

## Disclosure

To maintain transparency and enable effective code review, contributors must disclose all generative AI usage. This is not meant to discourage AI use, but to ensure that maintainers and reviewers have the necessary context to evaluate contributions effectively. This includes contributions in the forms of Pull Requests, Issues, Security Advisories, Discussions, or any other communication channels.

### What to Disclose

Include information about:

- Type of assistance: Code generation, documentation, debugging, testing, refactoring, etc.
- Scope of usage: Which files, functions, or sections were AI-assisted
- Tool(s) used: Name of the AI system(s) employed (e.g., GitHub Copilot, ChatGPT, etc.)
- Level of modification: Whether AI-generated content was used as-is, modified, or used as inspiration

## Responsibility

- Review all AI-generated code thoroughly before submission
- Verify necessity and relevance - Remove verbose or unnecessary AI-generated content
- Be concise - Edit AI output to be clear and to-the-point
- Verify correctness and test all AI-assisted implementations
- Maintain authorship responsibility - you are accountable for all submitted code regardless of its origin

### Security

- Be cautious with external dependencies suggested by AI tools
- Validate security implications of AI-generated code, especially for flight software

### Code Review Considerations

- Provide context to reviewers about AI usage to enable informed evaluation
- Be prepared to explain AI-generated logic and design decisions
- Accept feedback gracefully - AI-generated code is not exempt from revision requests
- Document complex AI-assisted algorithms clearly for future maintainers
