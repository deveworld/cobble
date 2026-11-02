# Finite IEEE binary64 represented as sign * m * 2^e. All arithmetic here
# is integer arithmetic on limbs. No native float provider is used.

def _cobble_soft_round(numerator, denominator, exponent, sign, precision):
    minimum = -1074
    maximum = 1023
    if precision == 24:
        minimum = -149
        maximum = 127
    if denominator == [0]:
        return {"m": [0], "e": 0, "sign": sign, "error": True}
    if numerator == [0]:
        return {"m": [0], "e": 0, "sign": sign, "error": False}
    power = _cobble_big_bits(numerator) - _cobble_big_bits(denominator)
    if power >= 0:
        trial = _cobble_big_shl(denominator, power)
        comparison = _cobble_big_compare(numerator, trial)
    else:
        trial = _cobble_big_shl(numerator, -power)
        comparison = _cobble_big_compare(trial, denominator)
    if comparison < 0:
        power -= 1
    if power + exponent > maximum:
        return {"m": [0], "e": 0, "sign": sign, "error": True}
    output_exponent = power + exponent - precision + 1
    if output_exponent < minimum:
        output_exponent = minimum
    shift = exponent - output_exponent
    if shift >= 0:
        numerator = _cobble_big_shl(numerator, shift)
    else:
        denominator = _cobble_big_shl(denominator, -shift)
    divided = _cobble_big_divmod(numerator, denominator)
    mantissa = divided.q
    twice = _cobble_big_mul_small(divided.r, 2)
    comparison = _cobble_big_compare(twice, denominator)
    if comparison > 0 or (comparison == 0 and mantissa[0] % 2 == 1):
        mantissa = _cobble_big_add(mantissa, [1])
    if _cobble_big_bits(mantissa) > precision:
        mantissa = _cobble_big_shr(mantissa, 1)
        output_exponent += 1
    if _cobble_big_bits(mantissa) + output_exponent - 1 > maximum:
        return {"m": [0], "e": 0, "sign": sign, "error": True}
    return {"m": mantissa, "e": output_exponent, "sign": sign, "error": False}

def _cobble_soft_parse(text, precision):
    sign = 1
    if text[0] == "-":
        sign = -1
        text = text[1:]
    elif text[0] == "+":
        text = text[1:]
    if text[-1] == "d" or text[-1] == "D" or text[-1] == "f" or text[-1] == "F" or text[-1] == "b" or text[-1] == "s" or text[-1] == "L":
        text = text[:-1]
    digits = ""
    fractional = 0
    after_point = 0
    after_exponent = 0
    exponent_sign = 1
    decimal_exponent = 0
    seen_digit = 0
    exponent_digits = 0
    exponent_signed = 0
    i = 0
    while i < len(text):
        ch = text[i]
        if ch == "." and after_point == 0 and after_exponent == 0:
            after_point = 1
        elif (ch == "e" or ch == "E") and after_exponent == 0 and seen_digit > 0:
            after_exponent = 1
        elif (ch == "+" or ch == "-") and after_exponent == 1 and exponent_digits == 0 and exponent_signed == 0:
            exponent_signed = 1
            if ch == "-":
                exponent_sign = -1
        else:
            digit = -1
            if ch == "0":
                digit = 0
            elif ch == "1":
                digit = 1
            elif ch == "2":
                digit = 2
            elif ch == "3":
                digit = 3
            elif ch == "4":
                digit = 4
            elif ch == "5":
                digit = 5
            elif ch == "6":
                digit = 6
            elif ch == "7":
                digit = 7
            elif ch == "8":
                digit = 8
            elif ch == "9":
                digit = 9
            if digit < 0:
                return {"m": [0], "e": 0, "sign": sign, "error": True}
            if after_exponent == 1:
                decimal_exponent = decimal_exponent * 10 + digit
                exponent_digits += 1
                if decimal_exponent > 2048:
                    return {"m": [0], "e": 0, "sign": sign, "error": True}
            else:
                digits += ch
                seen_digit += 1
                if after_point == 1:
                    fractional += 1
        i += 1
    if seen_digit == 0 or (after_exponent == 1 and exponent_digits == 0):
        return {"m": [0], "e": 0, "sign": sign, "error": True}
    exponent = decimal_exponent * exponent_sign - fractional
    numerator = _cobble_big_from_decimal(digits)
    denominator = [1]
    if exponent > 0:
        numerator = _cobble_big_mul(numerator, _cobble_big_pow10(exponent))
    elif exponent < 0:
        denominator = _cobble_big_pow10(-exponent)
    return _cobble_soft_round(numerator, denominator, 0, sign, precision)

def _cobble_soft_add(a, b):
    if a.error:
        return a
    if b.error:
        return b
    exponent = a.e
    if b.e < exponent:
        exponent = b.e
    left = _cobble_big_shl(a.m, a.e - exponent)
    right = _cobble_big_shl(b.m, b.e - exponent)
    sign = a.sign
    if a.sign == b.sign:
        mantissa = _cobble_big_add(left, right)
    elif _cobble_big_compare(left, right) >= 0:
        mantissa = _cobble_big_sub(left, right)
        if mantissa == [0]:
            sign = 1
    else:
        mantissa = _cobble_big_sub(right, left)
        sign = b.sign
    return _cobble_soft_round(mantissa, [1], exponent, sign, 53)

def _cobble_soft_sub(a, b):
    opposite = {"m": b.m, "e": b.e, "sign": -b.sign, "error": b.error}
    return _cobble_soft_add(a, opposite)

def _cobble_soft_mul(a, b):
    if a.error:
        return a
    if b.error:
        return b
    mantissa = _cobble_big_mul(a.m, b.m)
    return _cobble_soft_round(mantissa, [1], a.e + b.e, a.sign * b.sign, 53)

def _cobble_soft_div(a, b):
    if a.error:
        return a
    if b.error:
        return b
    return _cobble_soft_round(a.m, b.m, a.e - b.e, a.sign * b.sign, 53)

def _cobble_soft_mod(a, b):
    if a.error:
        return a
    if b.error:
        return b
    exponent = a.e
    if b.e < exponent:
        exponent = b.e
    left = _cobble_big_shl(a.m, a.e - exponent)
    right = _cobble_big_shl(b.m, b.e - exponent)
    if right == [0]:
        return {"m": [0], "e": 0, "sign": 1, "error": True}
    division = _cobble_big_divmod(left, right)
    remainder = division.r
    if a.sign != b.sign and remainder != [0]:
        remainder = _cobble_big_sub(right, remainder)
    return _cobble_soft_round(remainder, [1], exponent, b.sign, 53)

def _cobble_soft_compare(a, b):
    if a.m == [0] and b.m == [0]:
        return 0
    if a.sign != b.sign:
        return a.sign
    exponent = a.e
    if b.e < exponent:
        exponent = b.e
    left = _cobble_big_shl(a.m, a.e - exponent)
    right = _cobble_big_shl(b.m, b.e - exponent)
    return _cobble_big_compare(left, right) * a.sign

def _cobble_soft_pow(a, exponent):
    result = {"m": [1], "e": 0, "sign": 1, "error": False}
    if exponent < 0:
        a = _cobble_soft_div(result, a)
        if exponent == -2147483648:
            result = a
            exponent = 2147483647
        else:
            exponent = -exponent
    while exponent > 0:
        if exponent % 2 == 1:
            result = _cobble_soft_mul(result, a)
        exponent /= 2
        if exponent > 0:
            a = _cobble_soft_mul(a, a)
    return result

def _cobble_soft_decimal(value):
    if value.m == [0]:
        if value.sign < 0:
            return "-0"
        return "0"
    numerator = value.m
    denominator = [1]
    if value.e >= 0:
        numerator = _cobble_big_shl(numerator, value.e)
    else:
        denominator = _cobble_big_shl(denominator, -value.e)
    # This estimate differs from floor(log10(value)) by at most one over
    # the binary64 range. Correct it with an exact rational comparison.
    exponent = (_cobble_big_bits(value.m) + value.e - 1) * 30103 / 100000
    if exponent >= 0:
        trial = _cobble_big_mul(denominator, _cobble_big_pow10(exponent))
        comparison = _cobble_big_compare(numerator, trial)
    else:
        trial = _cobble_big_mul(numerator, _cobble_big_pow10(-exponent))
        comparison = _cobble_big_compare(trial, denominator)
    if comparison < 0:
        exponent -= 1
    else:
        if exponent + 1 >= 0:
            trial = _cobble_big_mul(denominator, _cobble_big_pow10(exponent + 1))
            comparison = _cobble_big_compare(numerator, trial)
        else:
            trial = _cobble_big_mul(numerator, _cobble_big_pow10(-exponent - 1))
            comparison = _cobble_big_compare(trial, denominator)
        if comparison >= 0:
            exponent += 1
    # A correctly rounded 17-digit decimal round-trips every binary64.
    shift = 16 - exponent
    if shift >= 0:
        numerator = _cobble_big_mul(numerator, _cobble_big_pow10(shift))
    else:
        denominator = _cobble_big_mul(denominator, _cobble_big_pow10(-shift))
    division = _cobble_big_divmod(numerator, denominator)
    mantissa = division.q
    comparison = _cobble_big_compare(_cobble_big_mul_small(division.r, 2), denominator)
    if comparison > 0 or (comparison == 0 and mantissa[0] % 2 == 1):
        mantissa = _cobble_big_add(mantissa, [1])
    digits = _cobble_big_to_decimal(mantissa)
    if value.sign < 0:
        digits = "-" + digits
    return digits + "e" + _cobble_format(exponent - 16)

def _cobble_soft_get_text(value):
    /data modify storage NS:global _cobble_result set value ""
    /data modify storage NS:global _cobble_result set string storage NS:global args.value

def _cobble_soft_decode(value):
    text = _cobble_soft_get_text(value)
    precision = 53
    if text[-1] == "f" or text[-1] == "F":
        precision = 24
    return _cobble_soft_parse(text, precision)

def _cobble_soft_emit_text(text):
    /data modify storage NS:global _cobble_result set value {text}d

def _cobble_soft_emit(value):
    text = _cobble_soft_decimal(value)
    _cobble_soft_emit_text(text)

def _cobble_numeric_double_add(left, right):
    a = _cobble_soft_decode(left)
    b = _cobble_soft_decode(right)
    out = _cobble_soft_add(a, b)
    if out.error:
        /tellraw @a {"text":"Invalid or overflowing double calculation.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    _cobble_soft_emit(out)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result

def _cobble_numeric_double_sub(left, right):
    a = _cobble_soft_decode(left)
    b = _cobble_soft_decode(right)
    out = _cobble_soft_sub(a, b)
    if out.error:
        /tellraw @a {"text":"Invalid or overflowing double calculation.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    _cobble_soft_emit(out)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result

def _cobble_numeric_double_mul(left, right):
    a = _cobble_soft_decode(left)
    b = _cobble_soft_decode(right)
    out = _cobble_soft_mul(a, b)
    if out.error:
        /tellraw @a {"text":"Invalid or overflowing double calculation.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    _cobble_soft_emit(out)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result

def _cobble_numeric_double_div(left, right):
    a = _cobble_soft_decode(left)
    b = _cobble_soft_decode(right)
    out = _cobble_soft_div(a, b)
    if out.error:
        /tellraw @a {"text":"Invalid or overflowing double calculation.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    _cobble_soft_emit(out)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result

def _cobble_numeric_double_mod(left, right):
    a = _cobble_soft_decode(left)
    b = _cobble_soft_decode(right)
    out = _cobble_soft_mod(a, b)
    if out.error:
        /tellraw @a {"text":"Invalid or overflowing double calculation.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    _cobble_soft_emit(out)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result

def _cobble_numeric_double_pow(left, right):
    a = _cobble_soft_decode(left)
    b = _cobble_soft_decode(right)
    integer = b.m
    if b.e < 0:
        integer = _cobble_big_shr(b.m, -b.e)
        restored = _cobble_big_shl(integer, -b.e)
        if _cobble_big_compare(restored, b.m) != 0:
            /tellraw @a {"text":"A double power currently needs an integer exponent.","color":"red"}
            /data modify storage NS:global _cobble_numeric.result set value 0.0d
            return 0
    elif b.e > 0:
        integer = _cobble_big_shl(b.m, b.e)
    limit = _cobble_big_from_int(2147483647)
    if b.sign < 0:
        limit = _cobble_big_add(limit, [1])
    if _cobble_big_compare(integer, limit) > 0:
        /tellraw @a {"text":"A double exponent must fit a 32-bit integer.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    exponent = 0
    i = len(integer) - 1
    while i >= 0:
        exponent = exponent * 32768 + integer[i]
        i -= 1
    exponent *= b.sign
    out = _cobble_soft_pow(a, exponent)
    if out.error:
        /tellraw @a {"text":"Invalid or overflowing double calculation.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    _cobble_soft_emit(out)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result

def _cobble_soft_floor_div(a, b):
    exponent = a.e
    if b.e < exponent:
        exponent = b.e
    left = _cobble_big_shl(a.m, a.e - exponent)
    right = _cobble_big_shl(b.m, b.e - exponent)
    if a.error or b.error or right == [0]:
        return {"m": [0], "e": 0, "sign": 1, "error": True}
    division = _cobble_big_divmod(left, right)
    quotient = division.q
    sign = a.sign * b.sign
    if sign < 0 and division.r != [0]:
        quotient = _cobble_big_add(quotient, [1])
    return _cobble_soft_round(quotient, [1], 0, sign, 53)

def _cobble_numeric_double_floor_div(left, right):
    out = _cobble_soft_floor_div(_cobble_soft_decode(left), _cobble_soft_decode(right))
    if out.error:
        /tellraw @a {"text":"Invalid or overflowing double calculation.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    _cobble_soft_emit(out)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result

def _cobble_numeric_double_compare(left, right):
    a = _cobble_soft_decode(left)
    b = _cobble_soft_decode(right)
    comparison = _cobble_soft_compare(a, b)
    /execute store result storage NS:global _cobble_numeric.result int 1 run scoreboard players get comparison temp

def _cobble_soft_emit_float_text(text):
    /data modify storage NS:global _cobble_result set value {text}f

def _cobble_numeric_double_cast_float(left, right):
    is_number = 0
    /execute store success score is_number temp run data get storage NS:global args.left 1
    if is_number == 0:
        text = _cobble_soft_get_text(left)
        rounded = _cobble_soft_parse(text, 24)
    else:
        value = _cobble_soft_decode(left)
        rounded = _cobble_soft_round(value.m, [1], value.e, value.sign, 24)
        if value.error:
            rounded.error = True
    if rounded.error:
        /tellraw @a {"text":"The value does not fit in a finite float.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0f
        return 0
    text = _cobble_soft_decimal(rounded)
    _cobble_soft_emit_float_text(text)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result

def _cobble_numeric_double_cast_double(left, right):
    value = _cobble_soft_decode(left)
    if value.error:
        /tellraw @a {"text":"The value does not fit in a finite double.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    _cobble_soft_emit(value)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result

def _cobble_numeric_double_cast_int(left, right):
    value = _cobble_soft_decode(left)
    integer = value.m
    if value.e < 0:
        integer = _cobble_big_shr(integer, -value.e)
    elif value.e > 0:
        integer = _cobble_big_shl(integer, value.e)
    limit = _cobble_big_from_int(2147483647)
    if value.sign < 0:
        limit = _cobble_big_add(limit, [1])
    if value.error or _cobble_big_compare(integer, limit) > 0:
        /tellraw @a {"text":"The value does not fit in a 32-bit integer.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0
        return 0
    result = 0
    i = len(integer) - 1
    while i >= 0:
        result = result * 32768 + integer[i]
        i -= 1
    result *= value.sign
    /execute store result storage NS:global _cobble_numeric.result int 1 run scoreboard players get result temp

def _cobble_soft_sqrt(value):
    if value.error or (value.sign < 0 and value.m != [0]):
        return {"m": [0], "e": 0, "sign": 1, "error": True}
    if value.m == [0]:
        return value
    power = _cobble_big_bits(value.m) + value.e - 1
    exponent = power / 2 - 52
    numerator = _cobble_big_shl(value.m, value.e - 2 * exponent)
    mantissa = _cobble_big_sqrt(numerator)
    midpoint = _cobble_big_add(_cobble_big_mul_small(mantissa, 2), [1])
    midpoint = _cobble_big_mul(midpoint, midpoint)
    comparison = _cobble_big_compare(_cobble_big_mul_small(numerator, 4), midpoint)
    if comparison > 0:
        mantissa = _cobble_big_add(mantissa, [1])
    return _cobble_soft_round(mantissa, [1], exponent, 1, 53)

def _cobble_numeric_double_sqrt(left, right):
    out = _cobble_soft_sqrt(_cobble_soft_decode(left))
    if out.error:
        /tellraw @a {"text":"A square root needs a nonnegative number.","color":"red"}
        /data modify storage NS:global _cobble_numeric.result set value 0.0d
        return 0
    _cobble_soft_emit(out)
    /data modify storage NS:global _cobble_numeric.result set from storage NS:global _cobble_result
