export const newline = /[\r\n\u0085\u2028\u2029]/u;
export const newlineMessage = "主密码和密码不允许换行。";
/** Inspect the original paste/drop/insertion payload before input normalization. */
export function guardForm(
  form: HTMLFormElement,
  fields: readonly string[],
  reject: (message: string) => void,
): () => void {
  const listener = (event: Event) => {
    const target = event.target;
    if (!(target instanceof HTMLInputElement) || !fields.includes(target.name))
      return;
    const value =
      event.type === "paste"
        ? (event as ClipboardEvent).clipboardData?.getData("text/plain")
        : event.type === "drop"
          ? (event as DragEvent).dataTransfer?.getData("text/plain")
          : ((event as InputEvent).data ??
            (event as InputEvent).dataTransfer?.getData("text/plain"));
    if (value && newline.test(value)) {
      event.preventDefault();
      form.reset();
      // reset() restores default accounts; rejected sensitive fields must be empty.
      for (const field of fields) {
        const input = form.elements.namedItem(field);
        if (input instanceof HTMLInputElement) input.value = "";
      }
      reject(newlineMessage);
    }
  };
  for (const type of ["paste", "drop", "beforeinput"])
    form.addEventListener(type, listener, true);
  return () => {
    for (const type of ["paste", "drop", "beforeinput"])
      form.removeEventListener(type, listener, true);
  };
}
