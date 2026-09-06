import {
  Children,
  isValidElement,
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ChangeEvent,
  type KeyboardEvent,
  type ReactNode,
  type SelectHTMLAttributes,
} from "react";
import { createPortal } from "react-dom";
import { Icon } from "./Icon";

export type SelectProps = SelectHTMLAttributes<HTMLSelectElement>;

interface OptionData {
  value: string;
  label: string;
  disabled: boolean;
}

function optionsFromChildren(children: ReactNode): OptionData[] {
  const out: OptionData[] = [];
  Children.forEach(children, (child) => {
    if (!isValidElement(child) || child.type !== "option") return;
    const props = child.props as {
      value?: string | number;
      disabled?: boolean;
      children?: ReactNode;
    };
    out.push({
      value: props.value == null ? "" : String(props.value),
      label: typeof props.children === "string" || typeof props.children === "number" ? String(props.children) : "",
      disabled: Boolean(props.disabled),
    });
  });
  return out;
}

/**
 * Themed select. Native `<select>` always pops the macOS system menu, which
 * cannot follow Aside's type, color, or sharp corners. This keeps the same
 * `value` / `onChange` / `<option>` API and draws an L3 menu instead.
 */
export function Select({ className, children, value, disabled, onChange, id, name }: SelectProps) {
  const options = optionsFromChildren(children);
  const selected = options.find((option) => option.value === String(value ?? "")) ?? null;
  const placeholder = options.find((option) => option.disabled && option.value === "") ?? null;
  const menuOptions = options.filter((option) => !(option.disabled && option.value === ""));

  const [open, setOpen] = useState(false);
  const [menuBox, setMenuBox] = useState<{ top: number; left: number; width: number } | null>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const listId = useId();

  const close = () => setOpen(false);

  const pick = (next: string) => {
    onChange?.({
      target: { value: next, name: name ?? "" },
      currentTarget: { value: next, name: name ?? "" },
    } as ChangeEvent<HTMLSelectElement>);
    close();
  };

  useLayoutEffect(() => {
    if (!open || !triggerRef.current) return;
    const rect = triggerRef.current.getBoundingClientRect();
    const estimated = Math.min(240, 8 + menuOptions.length * 36);
    const spaceBelow = window.innerHeight - rect.bottom;
    const top = spaceBelow < estimated && rect.top > estimated ? rect.top - estimated - 4 : rect.bottom + 4;
    setMenuBox({ top, left: rect.left, width: Math.max(rect.width, 180) });
  }, [open]);

  useEffect(() => {
    if (!open) return;
    function onPointerDown(event: PointerEvent) {
      const target = event.target as Node | null;
      if (triggerRef.current?.contains(target)) return;
      if (menuRef.current?.contains(target)) return;
      close();
    }
    function onKey(event: globalThis.KeyboardEvent) {
      if (event.key === "Escape") {
        event.stopPropagation();
        close();
        triggerRef.current?.focus();
      }
    }
    document.addEventListener("pointerdown", onPointerDown);
    document.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      document.removeEventListener("keydown", onKey, true);
    };
  }, [open]);

  const onTriggerKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      setOpen(true);
    }
  };

  const label = selected?.label || placeholder?.label || "Choose one";
  const isPlaceholder = !selected || selected.value === "";
  const classes = ["u-select", className].filter(Boolean).join(" ");
  const host = typeof document !== "undefined" ? document.querySelector(".aw-root") : null;

  return (
    <div className={classes}>
      <button
        ref={triggerRef}
        type="button"
        id={id}
        disabled={disabled}
        className="u-select__trigger"
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={listId}
        onClick={() => setOpen((current) => !current)}
        onKeyDown={onTriggerKeyDown}
      >
        <span className={isPlaceholder ? "u-select__value u-select__value--placeholder" : "u-select__value"}>
          {label}
        </span>
        <Icon name="chevronDown" size={12} className="u-select__caret" />
      </button>
      {open && menuBox && host
        ? createPortal(
            <div
              ref={menuRef}
              id={listId}
              role="listbox"
              className="u-select__menu"
              style={{ top: menuBox.top, left: menuBox.left, width: menuBox.width }}
            >
              {menuOptions.map((option) => {
                const isSelected = option.value === String(value ?? "");
                return (
                  <button
                    key={option.value || option.label}
                    type="button"
                    role="option"
                    aria-selected={isSelected}
                    disabled={option.disabled}
                    className={[
                      "u-select__option",
                      isSelected ? "u-select__option--selected" : null,
                    ]
                      .filter(Boolean)
                      .join(" ")}
                    onClick={() => {
                      if (!option.disabled) pick(option.value);
                    }}
                  >
                    {option.label}
                  </button>
                );
              })}
            </div>,
            host,
          )
        : null}
    </div>
  );
}
