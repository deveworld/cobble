# Helpers behind Python forms that need a loop or a macro. Each one is
# compiled into a pack only when the pack uses it. `NS` is the pack's
# namespace. A result that isn't a number goes to the storage path
# `_cobble_result`.

def _cobble_list_sum(items):
    total = 0
    for item in items:
        total += item
    return total

def _cobble_list_min(items):
    best = items[0]
    for item in items:
        if item < best:
            best = item
    return best

def _cobble_list_max(items):
    best = items[0]
    for item in items:
        if item > best:
            best = item
    return best

def _cobble_list_index(items, value):
    position = 0
    for item in items:
        if item == value:
            return position
        position += 1
    return -1

def _cobble_list_count(items, value):
    found = 0
    for item in items:
        if item == value:
            found += 1
    return found

def _cobble_list_remove(items, value):
    out = []
    removed = 0
    for item in items:
        if removed == 0 and item == value:
            removed = 1
        else:
            out.append(item)
    /data modify storage NS:global _cobble_result set from storage NS:global locals._cobble_list_remove.out

def _cobble_list_insert_at(items, position, value):
    /data modify storage NS:global args.items insert {position} from storage NS:global args.value
    /data modify storage NS:global _cobble_result set from storage NS:global args.items

def _cobble_list_insert(items, position, value):
    size = len(items)
    spot = position
    if spot < 0:
        spot += size
        if spot < 0:
            spot = 0
    if spot > size:
        spot = size
    _cobble_list_insert_at(items, spot, value)

def _cobble_list_reverse(items):
    out = []
    position = len(items) - 1
    while position >= 0:
        out.append(items[position])
        position -= 1
    /data modify storage NS:global _cobble_result set from storage NS:global locals._cobble_list_reverse.out

def _cobble_list_sort(items):
    out = []
    for item in items:
        spot = len(out)
        moving = 1
        while moving == 1 and spot > 0:
            if out[spot - 1] > item:
                spot -= 1
            else:
                moving = 0
        _cobble_list_insert_at(out, spot, item)
        /data modify storage NS:global locals._cobble_list_sort.out set from storage NS:global _cobble_result
    /data modify storage NS:global _cobble_result set from storage NS:global locals._cobble_list_sort.out

def _cobble_list_repeat(items, count):
    out = []
    done = 0
    while done < count:
        out.extend(items)
        done += 1
    /data modify storage NS:global _cobble_result set from storage NS:global locals._cobble_list_repeat.out

def _cobble_list_slice(items, start, stop):
    size = len(items)
    first = start
    last = stop
    if first < 0:
        first += size
        if first < 0:
            first = 0
    if last < 0:
        last += size
    if last > size:
        last = size
    out = []
    while first < last:
        out.append(items[first])
        first += 1
    /data modify storage NS:global _cobble_result set from storage NS:global locals._cobble_list_slice.out

def _cobble_text_cut(text, start, stop):
    /data modify storage NS:global _cobble_result set string storage NS:global args.text {start} {stop}

def _cobble_text_slice(text, start, stop):
    size = len(text)
    first = start
    last = stop
    if first < 0:
        first += size
        if first < 0:
            first = 0
    if last < 0:
        last += size
    if last > size:
        last = size
    if first < last:
        _cobble_text_cut(text, first, last)
    else:
        /data modify storage NS:global _cobble_result set value ""

# Joining, repeating, `str`, and map keys pass text through a macro line,
# which reads backslashes and quotes again; the `_cobble_esc_` functions
# (text_escape.rs) escape it first.
def _cobble_text_join(left, right):
    /function NS:_cobble_esc_join

def _cobble_text_repeat(text, count):
    /function NS:_cobble_esc_repeat

def _cobble_text_find(text, part):
    size = len(part)
    last = len(text) - size
    position = 0
    while position <= last:
        _cobble_text_cut(text, position, position + size)
        if _cobble_result == part:
            return position
        position += 1
    return -1

def _cobble_str(value):
    /function NS:_cobble_esc_str

def _cobble_format_value(value):
    /data modify storage NS:global _cobble_result set value ""
    /data remove storage NS:global _cobble_format_probe
    /execute store success score #format_ok temp run data modify storage NS:global _cobble_format_probe set string storage NS:global args.value
    /execute if score #format_ok temp matches 0 run tellraw @a {"text":"An f-string value must be text or a number.","color":"red"}
    /execute if score #format_ok temp matches 0 run return 0
    /function NS:_cobble_esc_str

def _cobble_has_key(box, key):
    # A number is never a key. Its text differs from it, so the second
    # `set` succeeds; for text it changes nothing and fails.
    number = 1
    /data remove storage NS:global _cobble_kind
    /data modify storage NS:global _cobble_kind set string storage NS:global args.key
    /execute store success score number temp run data modify storage NS:global _cobble_kind set from storage NS:global args.key
    if number == 1:
        return 0
    found = 0
    /execute store result score found temp run function NS:_cobble_esc_has
    return found

def _cobble_is_text(value):
    found = 0
    /data remove storage NS:global _cobble_kind
    /execute store success score found temp run data modify storage NS:global _cobble_kind set string storage NS:global args.value
    return found

def _cobble_is_list(value):
    /execute if data storage NS:global args.value[] run return 1
    return 0

def _cobble_contains(box, value):
    if _cobble_is_text(box):
        return _cobble_text_find(box, value) >= 0
    if _cobble_is_list(box):
        return _cobble_list_index(box, value) >= 0
    return _cobble_has_key(box, value)

def _cobble_join(left, right):
    # An empty list has no item to find, so a list is whatever isn't text.
    if not _cobble_is_text(left):
        /data modify storage NS:global _cobble_result set from storage NS:global args.left
        /data modify storage NS:global _cobble_result append from storage NS:global args.right[]
    else:
        _cobble_text_join(left, right)

def _cobble_kind(value):
    # 0 for a number, 1 for text, 2 for a list or a map. A list or a map
    # has no text, so the first `set` fails; a number's text differs from
    # it, so the second `set` succeeds, and for text it changes nothing.
    kind = 0
    /data remove storage NS:global _cobble_kind
    /execute store success score kind temp run data modify storage NS:global _cobble_kind set string storage NS:global args.value
    if kind == 0:
        return 2
    /execute store success score kind temp run data modify storage NS:global _cobble_kind set from storage NS:global args.value
    return 1 - kind

# `+` and `*` on values whose type only the running pack knows: text and
# lists join and repeat, and numbers add and multiply.
def _cobble_add(left, right):
    kind = _cobble_kind(left)
    if kind == 0:
        out = [left + right]
        /data modify storage NS:global _cobble_result set from storage NS:global locals._cobble_add.out[0]
    elif kind == 1:
        _cobble_text_join(left, right)
    else:
        /data modify storage NS:global _cobble_result set from storage NS:global args.left
        /data modify storage NS:global _cobble_result append from storage NS:global args.right[]

def _cobble_repeat(value, count, kind):
    if kind == 1:
        _cobble_text_repeat(value, count)
    else:
        _cobble_list_repeat(value, count)

def _cobble_mul(left, right):
    kind = _cobble_kind(left)
    if kind != 0:
        _cobble_repeat(left, right, kind)
        return 0
    kind = _cobble_kind(right)
    if kind != 0:
        _cobble_repeat(right, left, kind)
        return 0
    out = [left * right]
    /data modify storage NS:global _cobble_result set from storage NS:global locals._cobble_mul.out[0]
    return 0

def _cobble_slice(value, start, stop):
    if _cobble_is_text(value):
        _cobble_text_slice(value, start, stop)
    else:
        _cobble_list_slice(value, start, stop)
