# Component composition

## Required structure

- `SelectItem` belongs in `SelectGroup`; equivalent menu and command items stay
  in their documented group.
- `Dialog`, `AlertDialog`, `Sheet`, and `Drawer` include a title. A visually
  hidden title is acceptable when the design supplies equivalent context.
- `Card` uses `CardHeader`, `CardTitle`, optional `CardDescription`,
  `CardContent`, and `CardFooter`/actions where applicable.
- `TabsTrigger` belongs in `TabsList`; panel content belongs in `TabsContent`.
- `Avatar` includes `AvatarFallback` even when an image is supplied.
- Use `Separator`, `Skeleton`, `Badge`, `Empty`, `Alert`, `Status`, and
  `Spinner` rather than rebuilding those visual/semantic states with generic
  elements.

## Forms

- Compose forms from `FieldGroup`, `Field`, `FieldLabel`, `FieldDescription`,
  and `FieldError`; use `FieldSet`/`FieldLegend` for related choices.
- Match control semantics: `Input`, `Textarea`, `Select`, `Combobox`,
  `Checkbox`, `RadioGroup`, `Switch`, `ToggleGroup`, `InputOtp`, or
  `InputPhone`.
- Connect labels, descriptions, validation, disabled state, and submitted names.
  Styling state does not replace `aria-invalid`, `disabled`, or server-side
  validation.
- `InputGroup` uses its documented input/textarea and addon parts; do not
  absolutely position arbitrary buttons over inputs.

## Data and communication

- Use `DataTable` for a semantic table with modest interaction and `DataGrid`
  for virtualization, pinning, selection, editing, or large datasets.
- Use `Message`, `Bubble`, `Attachment`, and `Marker` for conversation surfaces.
- Use `Toast`/`Sonner` for transient feedback and `Alert`/`Callout` for feedback
  that must remain in the document.
