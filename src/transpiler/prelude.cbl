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

def _cobble_text_join(left, right):
    /data modify storage NS:global _cobble_result set value "{left}{right}"

def _cobble_text_repeat(text, count):
    out = ""
    done = 0
    while done < count:
        _cobble_text_join(out, text)
        /data modify storage NS:global locals._cobble_text_repeat.out set from storage NS:global _cobble_result
        done += 1
    /data modify storage NS:global _cobble_result set from storage NS:global locals._cobble_text_repeat.out

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
    /data modify storage NS:global _cobble_result set value "{value}"

def _cobble_has_key(box, key):
    /execute if data storage NS:global args.box."{key}" run return 1
    return 0

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

def _cobble_slice(value, start, stop):
    if _cobble_is_text(value):
        _cobble_text_slice(value, start, stop)
    else:
        _cobble_list_slice(value, start, stop)
