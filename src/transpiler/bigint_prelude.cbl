# Unsigned little-endian base-32768 limbs. Products and carries fit i32.
# These helpers are compiled with integer arithmetic, including for the
# software binary64 runtime. Their non-integer results use _cobble_result.

def _cobble_big_trim(a):
    while len(a) > 1 and a[-1] == 0:
        a.pop()
    if not a:
        a.append(0)
    return a

def _cobble_big_compare(a, b):
    a = _cobble_big_trim(a)
    b = _cobble_big_trim(b)
    if len(a) > len(b):
        return 1
    if len(a) < len(b):
        return -1
    i = len(a) - 1
    while i >= 0:
        if a[i] > b[i]:
            return 1
        if a[i] < b[i]:
            return -1
        i -= 1
    return 0

def _cobble_big_bits(a):
    a = _cobble_big_trim(a)
    top = a[-1]
    bits = (len(a) - 1) * 15
    while top > 0:
        top /= 2
        bits += 1
    return bits

def _cobble_big_from_int(n):
    low = n % 32768
    high = n / 32768
    if n < 0:
        low = -low
        high = -high
        if low < 0:
            low += 32768
            high -= 1
    out = [low]
    while high > 0:
        out.append(high % 32768)
        high /= 32768
    return out

def _cobble_big_add(a, b):
    out = []
    carry = 0
    i = 0
    size = len(a)
    if len(b) > size:
        size = len(b)
    while i < size:
        value = carry
        if i < len(a):
            value += a[i]
        if i < len(b):
            value += b[i]
        out.append(value % 32768)
        carry = value / 32768
        i += 1
    if carry > 0:
        out.append(carry)
    return _cobble_big_trim(out)

def _cobble_big_sub(a, b):
    out = []
    borrow = 0
    i = 0
    while i < len(a):
        value = a[i] - borrow
        if i < len(b):
            value -= b[i]
        borrow = 0
        if value < 0:
            value += 32768
            borrow = 1
        out.append(value)
        i += 1
    return _cobble_big_trim(out)

def _cobble_big_mul_small(a, n):
    out = []
    carry = 0
    i = 0
    while i < len(a):
        value = a[i] * n + carry
        out.append(value % 32768)
        carry = value / 32768
        i += 1
    while carry > 0:
        out.append(carry % 32768)
        carry /= 32768
    return _cobble_big_trim(out)

def _cobble_big_mul(a, b):
    out = []
    i = 0
    while i < len(a) + len(b) + 1:
        out.append(0)
        i += 1
    i = 0
    while i < len(a):
        carry = 0
        j = 0
        while j < len(b):
            at = i + j
            value = out[at] + a[i] * b[j] + carry
            out[at] = value % 32768
            carry = value / 32768
            j += 1
        at = i + len(b)
        while carry > 0:
            value = out[at] + carry
            out[at] = value % 32768
            carry = value / 32768
            at += 1
        i += 1
    return _cobble_big_trim(out)

def _cobble_big_shl(a, shift):
    words = shift / 15
    factor = 2 ^ (shift % 15)
    tail = _cobble_big_mul_small(a, factor)
    out = []
    i = 0
    while i < words:
        out.append(0)
        i += 1
    out.extend(tail)
    return _cobble_big_trim(out)

def _cobble_big_shr(a, shift):
    words = shift / 15
    factor = 2 ^ (shift % 15)
    out = []
    carry = 0
    i = len(a) - 1
    while i >= words:
        value = carry * 32768 + a[i]
        out.insert(0, value / factor)
        carry = value % factor
        i -= 1
    return _cobble_big_trim(out)

def _cobble_big_div_small(a, n):
    out = []
    remainder = 0
    i = len(a) - 1
    while i >= 0:
        value = remainder * 32768 + a[i]
        out.insert(0, value / n)
        remainder = value % n
        i -= 1
    return {"q": _cobble_big_trim(out), "r": remainder}

def _cobble_big_divmod(a, b):
    a = _cobble_big_trim(a)
    b = _cobble_big_trim(b)
    quotient = [0]
    shift = _cobble_big_bits(a) - _cobble_big_bits(b)
    if shift < 0 or b == [0]:
        return {"q": quotient, "r": a}
    divisor = _cobble_big_shl(b, shift)
    i = 0
    while i < shift / 15:
        quotient.append(0)
        i += 1
    while shift >= 0:
        if _cobble_big_compare(a, divisor) >= 0:
            a = _cobble_big_sub(a, divisor)
            at = shift / 15
            quotient[at] += 2 ^ (shift % 15)
        divisor = _cobble_big_shr(divisor, 1)
        shift -= 1
    return {"q": _cobble_big_trim(quotient), "r": a}

def _cobble_big_from_decimal(text):
    out = [0]
    i = 0
    while i < len(text):
        ch = text[i]
        digit = 0
        if ch == "1":
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
        out = _cobble_big_mul_small(out, 10)
        out = _cobble_big_add(out, [digit])
        i += 1
    return out

def _cobble_big_to_decimal(a):
    chunks = []
    while _cobble_big_compare(a, [0]) > 0:
        division = _cobble_big_div_small(a, 10000)
        chunks.append(division.r)
        a = division.q
    if not chunks:
        return "0"
    out = _cobble_format(chunks[-1])
    i = len(chunks) - 2
    while i >= 0:
        chunk = "0000" + _cobble_format(chunks[i])
        out += chunk[-4:]
        i -= 1
    return out

def _cobble_big_pow10(exponent):
    powers = [[10],[100],[10000],[24832,3051],[0,24450,7113,284],[0,0,15876,11623,5560,11688,15243,2],[0,0,0,0,28688,27971,22831,7286,28014,20271,21354,32719,16019,2528,6],[0,0,0,0,0,0,0,0,256,10876,9402,31944,20541,10015,17349,29207,9076,27608,20287,24646,16461,7000,13164,3294,13183,1868,31896,30536,36],[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,30722,24761,15068,27960,28247,13927,17388,5423,3108,31043,862,9959,31209,30211,27338,29038,19812,6848,28211,25162,8870,22151,8553,3644,32424,399,11166,31941,15842,6526,5241,21948,4078,883,28388,18158,31198,30523,31663,1363],[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,24580,25447,9855,32334,14109,19107,28559,11566,23926,21588,7043,17243,28871,649,18116,11918,21401,13836,2199,12089,7533,7586,30835,32337,30342,3408,27954,19975,29690,3858,12066,25652,27834,25231,30171,23593,27429,20982,11967,14511,19658,25482,24191,210,12543,3218,1449,18109,25981,30471,10504,15510,27371,5449,26227,14374,9693,29858,26032,3383,27627,14753,15489,24883,29978,11576,29828,19762,16616,22585,5004,5694,24104,23636,6346,18203,12404,2794,25396,56],[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,16,9022,23690,11669,23541,30786,6585,5372,29164,2512,9799,3396,5573,2666,20523,28880,18385,809,2870,1894,20804,23581,6355,23627,5839,28604,32135,3873,893,16285,11890,6377,23666,26570,13768,28208,2917,8771,21457,26249,25029,27440,29272,24015,4651,10339,8098,20541,31218,11091,12186,25137,4162,26908,28137,6178,25054,9047,2217,17772,17425,29801,21526,16549,27453,26619,1455,25229,15503,28973,12998,28535,24406,28031,19582,12965,14068,27693,16488,12248,14169,32717,1681,17646,22912,19201,30358,5125,648,21316,17369,6389,8080,31006,19523,15185,14313,19791,8888,13339,14559,18241,14579,4886,10468,27188,23912,23396,27824,27398,4453,10746,6998,28998,21944,21345,31403,16491,17390,4108,20181,29688,4678,17502,4426,11392,260,732,13792,7780,4733,18083,1566,20924,9434,10906,10044,22437,10745,19334,16806,24873,7265,20346,11700,27514,6927,27209,25097,4956,3301,12650,19004,1694,5989,27160,6661,13228,3223],[0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,256,26560,5196,19267,32304,10663,27807,18151,18914,3650,12473,6823,26750,12445,413,24027,2954,8813,5766,11567,22229,26074,7644,32449,20802,21665,14027,3516,18917,1669,12070,10349,29128,23981,18969,8544,29075,10544,9374,13537,21240,23701,6724,27771,7295,1454,27105,30123,21842,6214,32695,2011,25038,15984,10040,7184,13385,11815,1234,9736,19939,3466,10019,21524,17319,30412,19638,23335,7740,8160,24605,6004,25481,32553,17157,31888,3862,23253,2567,22996,31419,300,21585,27344,12222,4906,1937,2522,2499,24524,4186,5605,8698,8752,11580,2354,29947,30098,11846,16053,23916,8166,25490,24225,17433,11466,4761,10374,1509,1741,21843,19353,18822,32086,29831,26733,25745,18335,26905,19478,19623,6100,12783,28808,14505,14509,4574,25296,11871,12316,4627,20386,16810,25607,23747,22741,27223,28492,23883,25366,31357,16368,4433,12362,26116,24192,1680,27582,17269,31029,13819,29747,5616,26765,12821,2580,14696,16269,7912,18622,6615,24696,1519,8026,5555,16114,21957,22319,3948,4603,27676,25718,4698,22262,5945,11708,18049,13997,7083,29562,27630,2525,21878,3241,24124,17545,9986,27548,11225,28355,1620,19688,19407,15748,26490,13937,4311,22064,7215,30327,11861,16626,22497,27230,22233,24446,25121,16893,28506,2599,15631,8708,14490,26598,24144,32105,29788,12697,24857,16751,29679,11151,1552,2804,29737,31417,7724,8953,9892,30383,15926,56,19087,2009,11104,24996,11911,18614,25572,13825,23946,16765,10107,13886,18389,469,32389,5481,2112,16828,7667,15275,11615,14241,1980,24337,14738,29759,4639,26661,7401,28149,13488,14354,16239,24260,16546,12149,1925,15058,3986,28268,31429,22173,21610,24376,22726,12025,20191,1338,2228,5618,22668,25793,632,8678,23673,4371,22164,4549,6260,18759,28542,19348,15547,23710,7009,11350,18350,4320,25739,29746,2802,10855,30577,24681,31480,19421,27185,7332,19051,18033,29906,15508,20311,12002,2875,317]]
    out = [1]
    i = 0
    while exponent > 0:
        if exponent % 2 == 1:
            out = _cobble_big_mul(out, powers[i])
        exponent /= 2
        i += 1
    return out

def _cobble_big_sqrt(value):
    if value == [0]:
        return [0]
    estimate = _cobble_big_shl([1], (_cobble_big_bits(value) + 1) / 2)
    while True:
        divided = _cobble_big_divmod(value, estimate)
        next = _cobble_big_shr(_cobble_big_add(estimate, divided.q), 1)
        if _cobble_big_compare(next, estimate) >= 0:
            return estimate
        estimate = next
