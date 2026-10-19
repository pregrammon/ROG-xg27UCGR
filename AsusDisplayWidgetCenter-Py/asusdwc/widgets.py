"""Reusable tkinter widgets for the dark ROG-style UI."""

from __future__ import annotations

import math
import tkinter as tk
from tkinter import ttk

from .theme import COLORS, ui_font


# ---------------------------------------------------------------------------
# Small helpers
# ---------------------------------------------------------------------------
def _descendant(widget: tk.Misc, container: tk.Misc) -> bool:
    while widget is not None:
        if widget is container:
            return True
        try:
            widget = widget.master
        except Exception:
            return False
    return False


class Card(tk.Frame):
    def __init__(self, master, title=None, subtitle=None, padx=18, pady=16, **kwargs):
        super().__init__(
            master,
            bg=COLORS["card"],
            highlightbackground=COLORS["border"],
            highlightthickness=1,
            bd=0,
            **kwargs,
        )
        self._padx = padx
        self._pady = pady
        if title:
            header = tk.Frame(self, bg=COLORS["card"])
            header.pack(fill="x", padx=padx, pady=(pady, 12))
            tk.Label(
                header,
                text=title,
                bg=COLORS["card"],
                fg=COLORS["text"],
                font=ui_font(12, "bold"),
                anchor="w",
            ).pack(side="left")
            if subtitle:
                tk.Label(
                    header,
                    text=subtitle,
                    bg=COLORS["card"],
                    fg=COLORS["muted"],
                    font=ui_font(9),
                    anchor="w",
                ).pack(side="left", padx=(10, 0), pady=(2, 0))
            self.body = tk.Frame(self, bg=COLORS["card"])
            self.body.pack(fill="both", expand=True, padx=padx, pady=(0, pady))
        else:
            self.body = self
            self.configure(padx=0)

    def add(self, widget, **pack):
        widget.pack(**pack)
        return widget


class SectionHeader(tk.Frame):
    def __init__(self, master, title, subtitle=None, right_text=None):
        super().__init__(master, bg=COLORS["bg"])
        left = tk.Frame(self, bg=COLORS["bg"])
        left.pack(side="left", fill="x", expand=True)
        tk.Label(
            left, text=title, bg=COLORS["bg"], fg=COLORS["text"], font=ui_font(15, "bold")
        ).pack(anchor="w")
        if subtitle:
            tk.Label(
                left, text=subtitle, bg=COLORS["bg"], fg=COLORS["muted"], font=ui_font(9)
            ).pack(anchor="w", pady=(3, 0))
        if right_text:
            self.right = tk.Label(
                self, text=right_text, bg=COLORS["bg"], fg=COLORS["muted"], font=ui_font(9)
            )
            self.right.pack(side="right")


class NavItem(tk.Frame):
    def __init__(self, master, key, label, icon, command):
        super().__init__(master, bg=COLORS["sidebar"], height=44, cursor="hand2")
        self.pack_propagate(False)
        self.key = key
        self.command = command
        self._active = False
        self._hover = False
        self.bar = tk.Frame(self, width=3, bg=COLORS["sidebar"])
        self.bar.pack(side="left", fill="y")
        self.icon = tk.Canvas(
            self, width=22, height=22, bg=COLORS["sidebar"], highlightthickness=0
        )
        self.icon.pack(side="left", padx=(16, 10))
        self.label = tk.Label(
            self,
            text=label,
            bg=COLORS["sidebar"],
            fg=COLORS["text_dim"],
            font=ui_font(10),
            anchor="w",
            cursor="hand2",
        )
        self.label.pack(side="left", fill="both", expand=True)
        for widget in (self, self.icon, self.label):
            widget.bind("<Button-1>", self._click)
            widget.bind("<Enter>", self._enter)
            widget.bind("<Leave>", self._leave)
        self._paint()

    def _click(self, _event=None):
        self.command(self.key)

    def _enter(self, _event=None):
        self._hover = True
        self._paint()

    def _leave(self, event=None):
        if event is not None:
            target = self.winfo_containing(event.x_root, event.y_root)
            if target is not None and _descendant(target, self):
                return
        self._hover = False
        self._paint()

    def set_active(self, active: bool):
        self._active = active
        self._paint()

    def set_label(self, text: str):
        self.label.configure(text=text)

    def _paint(self):
        if self._active:
            bg = COLORS["card_active"]
            fg = COLORS["white"]
            bar = COLORS["accent"]
        elif self._hover:
            bg = COLORS["card"]
            fg = COLORS["text"]
            bar = COLORS["sidebar"]
        else:
            bg = COLORS["sidebar"]
            fg = COLORS["text_dim"]
            bar = COLORS["sidebar"]
        self.configure(bg=bg)
        self.icon.configure(bg=bg)
        self.label.configure(bg=bg, fg=fg)
        self.bar.configure(bg=bar)
        self.icon.delete("all")
        draw_icon(self.icon, self.key, COLORS["accent"] if self._active else fg, 22)


class PillButton(tk.Label):
    def __init__(self, master, text, command=None, accent=False, small=False, **kwargs):
        bg = COLORS["accent"] if accent else COLORS["panel"]
        fg = COLORS["white"] if accent else COLORS["text"]
        super().__init__(
            master,
            text=text,
            bg=bg,
            fg=fg,
            font=ui_font(9 if small else 10, "bold" if accent else "normal"),
            padx=12 if small else 16,
            pady=6 if small else 8,
            cursor="hand2",
            **kwargs,
        )
        self.command = command
        self._callback = command
        self._accent = accent
        self._hover = False
        self._enabled = True
        self.bind("<Button-1>", self._click)
        self.bind("<Enter>", self._enter)
        self.bind("<Leave>", self._leave)

    def _click(self, _event=None):
        if self.command:
            self.command()

    def _enter(self, _event=None):
        self._hover = True
        self.configure(bg=COLORS["accent_hover"] if self._accent else COLORS["card_hover"])

    def _leave(self, _event=None):
        self._hover = False
        self.configure(bg=COLORS["accent"] if self._accent else COLORS["panel"])

    def set_enabled(self, enabled: bool):
        self.configure(
            cursor="hand2" if enabled else "arrow",
            fg=(COLORS["white"] if self._accent else COLORS["text"]) if enabled else COLORS["muted"],
            bg=(COLORS["accent"] if self._accent else COLORS["panel"]) if enabled else COLORS["card"],
        )
        self.command = self._callback if enabled else None
        self._enabled = enabled


# ---------------------------------------------------------------------------
# Range slider
# ---------------------------------------------------------------------------
class SliderRow(tk.Frame):
    def __init__(
        self,
        master,
        label,
        minimum,
        maximum,
        value=0,
        command=None,
        suffix="",
        step=1,
        hint=None,
        fmt=None,
    ):
        super().__init__(master, bg=COLORS["card"])
        self.minimum = int(minimum)
        self.maximum = int(maximum)
        self.step = max(1, int(step))
        self.command = command
        self.suffix = suffix
        self.fmt = fmt or (lambda v: str(int(v)))
        self._enabled = True
        self._value = self._clamp(value)

        header = tk.Frame(self, bg=COLORS["card"])
        header.pack(fill="x")
        self.name_label = tk.Label(
            header, text=label, bg=COLORS["card"], fg=COLORS["text"], font=ui_font(10)
        )
        self.name_label.pack(side="left")
        self.value_label = tk.Label(
            header,
            text=self._text(self._value),
            bg=COLORS["card"],
            fg=COLORS["accent"],
            font=ui_font(10, "bold"),
        )
        self.value_label.pack(side="right")

        self.canvas = tk.Canvas(
            self, height=28, bg=COLORS["card"], highlightthickness=0, cursor="hand2"
        )
        self.canvas.pack(fill="x", pady=(2, 0))
        self.canvas.bind("<Configure>", lambda e: self._draw())
        self.canvas.bind("<Button-1>", self._on_press)
        self.canvas.bind("<B1-Motion>", self._on_drag)
        self.canvas.bind("<ButtonRelease-1>", self._on_release)
        self.canvas.bind("<Left>", lambda e: self._nudge(-self.step))
        self.canvas.bind("<Right>", lambda e: self._nudge(self.step))
        self.canvas.bind("<Home>", lambda e: self._keyboard_set(self.minimum))
        self.canvas.bind("<End>", lambda e: self._keyboard_set(self.maximum))
        self.canvas.configure(takefocus=1)
        self._dragging = False

        if hint:
            tk.Label(
                self, text=hint, bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8)
            ).pack(fill="x", pady=(2, 0))
        self._draw()

    # ------------------------------------------------------------------ api
    def set_value(self, value):
        if self._dragging:
            return
        self._value = self._clamp(value)
        self._update()

    def get_value(self) -> int:
        return self._value

    def set_range(self, minimum, maximum):
        self.minimum = int(minimum)
        self.maximum = max(self.minimum + 1, int(maximum))
        self._value = self._clamp(self._value)
        self._update()

    def set_enabled(self, enabled: bool):
        self._enabled = bool(enabled)
        self.canvas.configure(cursor="hand2" if enabled else "arrow")
        self.name_label.configure(fg=COLORS["text"] if enabled else COLORS["muted"])
        self.value_label.configure(fg=COLORS["accent"] if enabled else COLORS["muted"])
        self._draw()

    # -------------------------------------------------------------- internal
    def _clamp(self, value) -> int:
        value = int(round(float(value)))
        if value < self.minimum:
            value = self.minimum
        if value > self.maximum:
            value = self.maximum
        # snap
        if self.step > 1:
            value = self.minimum + round((value - self.minimum) / self.step) * self.step
            value = min(self.maximum, max(self.minimum, value))
        return value

    def _text(self, value) -> str:
        return f"{self.fmt(value)}{self.suffix}"

    def _ratio(self) -> float:
        span = max(1, self.maximum - self.minimum)
        return (self._value - self.minimum) / span

    def _from_x(self, x: int) -> int:
        pad = 10
        width = max(1, self.canvas.winfo_width() - pad * 2)
        ratio = min(1.0, max(0.0, (x - pad) / width))
        return self._clamp(self.minimum + ratio * (self.maximum - self.minimum))

    def _on_press(self, event):
        if not self._enabled:
            return
        self.canvas.focus_set()
        self._dragging = True
        self._value = self._from_x(event.x)
        self._update()

    def _on_drag(self, event):
        if not self._enabled or not self._dragging:
            return
        self._value = self._from_x(event.x)
        self._update()

    def _on_release(self, event):
        if not self._enabled or not self._dragging:
            return
        self._dragging = False
        self._value = self._from_x(event.x)
        self._update()
        self._commit()

    def _nudge(self, delta):
        if not self._enabled:
            return
        self._value = self._clamp(self._value + delta)
        self._update()
        self._commit()

    def _keyboard_set(self, value):
        if not self._enabled:
            return
        self._value = self._clamp(value)
        self._update()
        self._commit()

    def _commit(self):
        if self.command:
            self.command(self._value)

    def _update(self):
        self.value_label.configure(text=self._text(self._value))
        self._draw()

    def _draw(self):
        c = self.canvas
        c.delete("all")
        width = c.winfo_width()
        if width <= 1:
            width = 300
        height = int(c.cget("height"))
        y = height // 2 - 1
        pad = 10
        x0, x1 = pad, max(pad + 1, width - pad)
        c.create_line(x0, y, x1, y, fill=COLORS["track"], width=6, capstyle="round")
        xk = x0 + (x1 - x0) * self._ratio()
        if self._enabled:
            c.create_line(x0, y, xk, y, fill=COLORS["accent"], width=6, capstyle="round")
            c.create_oval(
                xk - 7, y - 7, xk + 7, y + 7, fill=COLORS["white"], outline=COLORS["accent"], width=2
            )
        else:
            c.create_oval(
                xk - 6, y - 6, xk + 6, y + 6, fill=COLORS["border"], outline=COLORS["border"]
            )


class ToggleRow(tk.Frame):
    def __init__(self, master, label, value=False, command=None, hint=None):
        super().__init__(master, bg=COLORS["card"])
        self.command = command
        self._value = bool(value)
        self._enabled = True
        row = tk.Frame(self, bg=COLORS["card"])
        row.pack(fill="x")
        self.name_label = tk.Label(
            row, text=label, bg=COLORS["card"], fg=COLORS["text"], font=ui_font(10)
        )
        self.name_label.pack(side="left")
        self.switch = tk.Canvas(
            row, width=44, height=24, bg=COLORS["card"], highlightthickness=0, cursor="hand2"
        )
        self.switch.pack(side="right")
        self.switch.bind("<Button-1>", self._toggle)
        if hint:
            tk.Label(
                self, text=hint, bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8),
                wraplength=560, justify="left", anchor="w",
            ).pack(fill="x", pady=(2, 0))
        self._draw()

    def set_state(self, value: bool):
        self._value = bool(value)
        self._draw()

    def get_state(self) -> bool:
        return self._value

    def set_enabled(self, enabled: bool):
        self._enabled = bool(enabled)
        self.name_label.configure(fg=COLORS["text"] if enabled else COLORS["muted"])
        self.switch.configure(cursor="hand2" if enabled else "arrow")
        self._draw()

    def _toggle(self, _event=None):
        if not self._enabled:
            return
        self._value = not self._value
        self._draw()
        if self.command:
            self.command(self._value)

    def _draw(self):
        c = self.switch
        c.delete("all")
        on = self._value and self._enabled
        track = COLORS["accent"] if on else COLORS["track"]
        if not self._enabled:
            track = COLORS["border_soft"]
        c.create_oval(1, 3, 21, 23, fill=track, outline=track)
        c.create_oval(23, 3, 43, 23, fill=track, outline=track)
        c.create_rectangle(11, 3, 33, 23, fill=track, outline=track)
        cx = 33 if self._value else 11
        knob = COLORS["white"] if self._enabled else COLORS["muted"]
        c.create_oval(cx - 8, 4, cx + 8, 20, fill=knob, outline=knob)


class Segmented(tk.Frame):
    def __init__(self, master, options=None, value=None, command=None, columns=0, small=False):
        super().__init__(master, bg=COLORS["card"])
        self.command = command
        self.columns = columns
        self.small = small
        self._buttons: list[tk.Label] = []
        self._items: list[tuple[str, object]] = []
        self._value = value
        self._enabled = True
        self.set_options(options or [], value)

    def set_options(self, options, value=None):
        for child in self.winfo_children():
            child.destroy()
        self._buttons = []
        self._items = list(options)
        for idx, (label, val) in enumerate(self._items):
            btn = tk.Label(
                self,
                text=label,
                bg=COLORS["panel"],
                fg=COLORS["text_dim"],
                font=ui_font(9 if self.small else 10),
                padx=10 if self.small else 14,
                pady=5 if self.small else 7,
                cursor="hand2",
            )
            if self.columns:
                btn.grid(row=idx // self.columns, column=idx % self.columns, padx=2, pady=2, sticky="ew")
            else:
                btn.pack(side="left", padx=(0, 4))
            btn.bind("<Button-1>", lambda e, v=val: self._click(v))
            btn.bind("<Enter>", lambda e, b=btn: self._hover(b, True))
            btn.bind("<Leave>", lambda e, b=btn: self._hover(b, False))
            self._buttons.append(btn)
        if self.columns:
            for col in range(self.columns):
                self.grid_columnconfigure(col, weight=1)
        self.set_value(value if value is not None else (self._items[0][1] if self._items else None), notify=False)

    def set_value(self, value, notify=False):
        self._value = value
        for (label, val), btn in zip(self._items, self._buttons):
            active = val == value
            btn.configure(
                bg=COLORS["accent"] if active else COLORS["panel"],
                fg=COLORS["white"] if active else COLORS["text_dim"],
                font=ui_font(9 if self.small else 10, "bold" if active else "normal"),
            )
        if notify and self.command:
            self.command(value)

    def get_value(self):
        return self._value

    def set_enabled(self, enabled: bool):
        self._enabled = bool(enabled)
        for btn in self._buttons:
            btn.configure(
                cursor="hand2" if enabled else "arrow",
                fg=COLORS["muted"] if not enabled else btn.cget("fg"),
            )

    def _click(self, value):
        if not self._enabled or value == self._value:
            return
        self.set_value(value, notify=True)

    def _hover(self, btn, entering: bool):
        if not self._enabled:
            return
        if btn.cget("bg") == COLORS["accent"]:
            return
        btn.configure(bg=COLORS["card_hover"] if entering else COLORS["panel"])


class ChoiceRow(tk.Frame):
    def __init__(self, master, label, options=None, value=None, command=None, hint=None):
        super().__init__(master, bg=COLORS["card"])
        self.command = command
        self._values: dict[str, object] = {}
        row = tk.Frame(self, bg=COLORS["card"])
        row.pack(fill="x")
        self.name_label = tk.Label(
            row, text=label, bg=COLORS["card"], fg=COLORS["text"], font=ui_font(10)
        )
        self.name_label.pack(side="left")
        self.combo = ttk.Combobox(row, state="readonly", width=18, font=ui_font(9))
        self.combo.pack(side="right")
        self.combo.bind("<<ComboboxSelected>>", self._changed)
        if hint:
            tk.Label(
                self, text=hint, bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8),
                wraplength=380, justify="left",
            ).pack(fill="x", pady=(2, 0))
        self.set_options(options or [], value)

    def set_options(self, options, value=None):
        self._values = {label: val for label, val in options}
        self.combo.configure(values=[label for label, _ in options])
        if value is not None:
            self.set_value(value)

    def set_value(self, value):
        for label, val in self._values.items():
            if val == value:
                self.combo.set(label)
                return
        self.combo.set("")

    def get_value(self):
        return self._values.get(self.combo.get())

    def set_enabled(self, enabled: bool):
        self.combo.configure(state="readonly" if enabled else "disabled")
        self.name_label.configure(fg=COLORS["text"] if enabled else COLORS["muted"])

    def _changed(self, _event=None):
        if self.command:
            self.command(self._values.get(self.combo.get()))


class ModeCard(tk.Frame):
    def __init__(self, master, title, subtitle, value, command, accent="#FF0050"):
        super().__init__(
            master,
            bg=COLORS["panel"],
            highlightbackground=COLORS["border"],
            highlightthickness=1,
            bd=0,
            cursor="hand2",
        )
        self.value = value
        self.command = command
        self.accent = accent
        self._active = False
        self._hover = False
        self.configure(height=84)
        self.pack_propagate(False)
        inner = tk.Frame(self, bg=COLORS["panel"])
        inner.pack(fill="both", expand=True, padx=14, pady=12)
        self.inner = inner
        self.title_label = tk.Label(
            inner, text=title, bg=COLORS["panel"], fg=COLORS["text"], font=ui_font(11, "bold"), anchor="w"
        )
        self.title_label.pack(anchor="w")
        self.sub_label = tk.Label(
            inner, text=subtitle, bg=COLORS["panel"], fg=COLORS["muted"], font=ui_font(8), anchor="w"
        )
        self.sub_label.pack(anchor="w", pady=(3, 0))
        self.marker = tk.Frame(self, width=3, bg=COLORS["panel"])
        self.marker.place(x=0, y=0, relheight=1)
        for widget in (self, inner, self.title_label, self.sub_label):
            widget.bind("<Button-1>", self._click)
            widget.bind("<Enter>", self._enter)
            widget.bind("<Leave>", self._leave)

    def _click(self, _event=None):
        self.command(self.value)

    def _enter(self, _event=None):
        self._hover = True
        self._paint()

    def _leave(self, event=None):
        if event is not None:
            target = self.winfo_containing(event.x_root, event.y_root)
            if target is not None and _descendant(target, self):
                return
        self._hover = False
        self._paint()

    def set_active(self, active: bool):
        self._active = bool(active)
        self._paint()

    def set_enabled(self, enabled: bool):
        self.configure(cursor="hand2" if enabled else "arrow")
        self.title_label.configure(fg=COLORS["text"] if enabled else COLORS["muted"])
        self.command = self.command if enabled else (lambda *_: None)

    def _paint(self):
        if self._active:
            bg = COLORS["card_active"]
            border = self.accent
        elif self._hover:
            bg = COLORS["card_hover"]
            border = COLORS["border_soft"]
        else:
            bg = COLORS["panel"]
            border = COLORS["border"]
        self.configure(bg=bg, highlightbackground=border)
        self.inner.configure(bg=bg)
        self.title_label.configure(bg=bg)
        self.sub_label.configure(bg=bg)
        self.marker.configure(bg=self.accent if self._active else bg)


class StatCard(tk.Frame):
    def __init__(self, master, label, value, sub=None):
        super().__init__(
            master,
            bg=COLORS["card"],
            highlightbackground=COLORS["border"],
            highlightthickness=1,
            bd=0,
        )
        inner = tk.Frame(self, bg=COLORS["card"])
        inner.pack(fill="both", expand=True, padx=16, pady=14)
        tk.Label(
            inner, text=label.upper(), bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8)
        ).pack(anchor="w")
        self.value_label = tk.Label(
            inner, text=value, bg=COLORS["card"], fg=COLORS["text"], font=ui_font(16, "bold")
        )
        self.value_label.pack(anchor="w", pady=(4, 0))
        self.sub_label = tk.Label(
            inner, text=sub or " ", bg=COLORS["card"], fg=COLORS["muted"], font=ui_font(8)
        )
        self.sub_label.pack(anchor="w", pady=(2, 0))

    def set_value(self, value, sub=None):
        self.value_label.configure(text=str(value))
        if sub is not None:
            self.sub_label.configure(text=sub)


class ScrollFrame(tk.Frame):
    def __init__(self, master, bg=None):
        bg = bg or COLORS["bg"]
        super().__init__(master, bg=bg)
        self.canvas = tk.Canvas(self, bg=bg, highlightthickness=0, bd=0)
        self.scrollbar = ttk.Scrollbar(self, orient="vertical", command=self.canvas.yview)
        self.canvas.configure(yscrollcommand=self.scrollbar.set)
        self.scrollbar.pack(side="right", fill="y")
        self.canvas.pack(side="left", fill="both", expand=True)
        self.inner = tk.Frame(self.canvas, bg=bg)
        self.window = self.canvas.create_window((0, 0), window=self.inner, anchor="nw")
        self.inner.bind("<Configure>", self._on_inner)
        self.canvas.bind("<Configure>", self._on_canvas)
        self.canvas.bind("<Enter>", self._bind_wheel)
        self.canvas.bind("<Leave>", self._unbind_wheel)
        self.inner.bind("<Enter>", self._bind_wheel)
        self.inner.bind("<Leave>", self._unbind_wheel)

    def _on_inner(self, _event=None):
        self.canvas.configure(scrollregion=self.canvas.bbox("all"))

    def _on_canvas(self, event):
        self.canvas.itemconfigure(self.window, width=event.width)

    def _bind_wheel(self, _event=None):
        self.canvas.bind_all("<MouseWheel>", self._wheel)

    def _unbind_wheel(self, _event=None):
        self.canvas.unbind_all("<MouseWheel>")

    def _wheel(self, event):
        if self.canvas.winfo_height() >= self.inner.winfo_reqheight():
            return
        delta = -1 if event.delta > 0 else 1
        self.canvas.yview_scroll(delta * 3, "units")

    def scroll_top(self):
        self.canvas.yview_moveto(0)


# ---------------------------------------------------------------------------
# Icons drawn with canvas primitives
# ---------------------------------------------------------------------------
def draw_icon(canvas: tk.Canvas, kind: str, color: str, size: int = 22) -> None:
    c = canvas
    c.delete("all")
    s = size / 22.0

    def line(x1, y1, x2, y2, width=1.6, cap="round"):
        c.create_line(x1 * s, y1 * s, x2 * s, y2 * s, fill=color, width=width, capstyle=cap)

    def oval(x1, y1, x2, y2, outline=color, width=1.6, fill=""):
        c.create_oval(x1 * s, y1 * s, x2 * s, y2 * s, outline=outline, width=width, fill=fill)

    def rect(x1, y1, x2, y2, outline=color, width=1.5, fill=""):
        c.create_rectangle(x1 * s, y1 * s, x2 * s, y2 * s, outline=outline, width=width, fill=fill)

    if kind == "overview":
        rect(2.5, 4, 19.5, 15, width=1.6)
        line(11, 15, 11, 18)
        line(6.5, 18, 15.5, 18)
        line(5, 8, 9, 8, 1.2)
        line(5, 11, 12, 11, 1.2)
    elif kind == "display":
        oval(7.5, 7.5, 14.5, 14.5, width=1.7)
        for angle in range(0, 360, 45):
            rad = math.radians(angle)
            x1 = 11 + 6.2 * math.cos(rad)
            y1 = 11 + 6.2 * math.sin(rad)
            x2 = 11 + 8.3 * math.cos(rad)
            y2 = 11 + 8.3 * math.sin(rad)
            line(x1, y1, x2, y2, 1.4)
    elif kind == "eyecare":
        c.create_arc(2 * s, 5 * s, 20 * s, 19 * s, start=200, extent=140, style="arc",
                     outline=color, width=1.6)
        c.create_arc(2 * s, 3 * s, 20 * s, 17 * s, start=20, extent=140, style="arc",
                     outline=color, width=1.6)
        oval(8.3, 8.3, 13.7, 13.7, width=1.5, fill=color)
    elif kind == "gameplus":
        oval(3.5, 3.5, 18.5, 18.5, width=1.5)
        line(11, 1.5, 11, 7)
        line(11, 15, 11, 20.5)
        line(1.5, 11, 7, 11)
        line(15, 11, 20.5, 11)
        oval(9.7, 9.7, 12.3, 12.3, width=1.2, fill=color)
    elif kind == "osd":
        rect(6, 2.5, 16, 19.5, width=1.6)
        oval(8, 5.5, 14, 11.5, width=1.3)
        line(11, 6.5, 11, 9, 1.2)
        line(9.7, 8, 12.3, 8, 1.2)
        oval(9, 14, 13, 18, width=1.2)
    elif kind == "profiles":
        c.create_polygon(11 * s, 3 * s, 19 * s, 7 * s, 11 * s, 11 * s, 3 * s, 7 * s,
                         outline=color, fill="", width=1.5)
        line(3, 11, 11, 15, 1.4)
        line(19, 11, 11, 15, 1.4)
        line(3, 15.5, 11, 19.5, 1.4)
        line(19, 15.5, 11, 19.5, 1.4)
    elif kind == "settings":
        oval(7.5, 7.5, 14.5, 14.5, width=1.6)
        oval(9.6, 9.6, 12.4, 12.4, width=1.4, fill=color)
        for angle in range(0, 360, 45):
            rad = math.radians(angle)
            x1 = 11 + 5.6 * math.cos(rad)
            y1 = 11 + 5.6 * math.sin(rad)
            x2 = 11 + 8.4 * math.cos(rad)
            y2 = 11 + 8.4 * math.sin(rad)
            line(x1, y1, x2, y2, 2.2)
    elif kind == "refresh":
        c.create_arc(4 * s, 4 * s, 18 * s, 18 * s, start=40, extent=280, style="arc",
                     outline=color, width=1.8)
        c.create_polygon(18 * s, 4 * s, 18 * s, 10 * s, 12.6 * s, 7 * s,
                         fill=color, outline=color)
    elif kind == "input":
        rect(3, 6, 12, 18, width=1.5)
        line(12, 11, 19, 11, 1.8)
        c.create_polygon(19 * s, 11 * s, 15 * s, 8 * s, 15 * s, 14 * s, fill=color, outline=color)
    elif kind == "power":
        c.create_arc(5 * s, 5 * s, 17 * s, 17 * s, start=300, extent=300, style="arc",
                     outline=color, width=1.8)
        line(11, 3, 11, 10.5, 1.8)
    elif kind == "monitor":
        rect(3, 4, 19, 15, width=1.5)
        line(11, 15, 11, 18)
        line(7, 18.5, 15, 18.5)
    elif kind == "close":
        line(6, 6, 16, 16, 1.8)
        line(16, 6, 6, 16, 1.8)
    else:
        oval(6, 6, 16, 16, width=1.5)

