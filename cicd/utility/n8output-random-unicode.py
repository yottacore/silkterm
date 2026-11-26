#!/usr/bin/env python3

##	Purpose: Spits out random lengths of random lines of unicode.
##	History: At bottom of script.

##	Copyright (c) 2026 Bubbles
##	Licensed under The MIT License (MIT). Full text at:
##		https://mit-license.org/
##	SPDX-License-Identifier: MIT


import sys, time, random, unicodedata

def usage():
	sys.stderr.write(f"usage: {sys.argv[0]} <duration_seconds> <delay_seconds>\n")
	sys.exit(1)

if len(sys.argv) != 3:
	usage()
try:
	duration = float(sys.argv[1])
	delay = float(sys.argv[2])
except ValueError:
	sys.stderr.write("error: arguments must be numbers\n")
	sys.exit(1)
if duration <= 0 or delay < 0:
	sys.stderr.write("error: duration must be > 0 and delay >= 0\n")
	sys.exit(1)

CHARS_MIN = 10           # min chars per line (incl spaces)
CHARS_MAX = 900          # max chars per line (incl spaces)
DELAY_JITTER_MIN = 0.25  # delay multiplier lower bound
DELAY_JITTER_MAX = 2.0   # delay multiplier upper bound
DELAY_JITTER_FLOOR = 0.1 # skip jitter when delay is below this value
WORD_GAP_MIN = 2         # min chars between spaces
WORD_GAP_MAX = 12        # max chars between spaces
PROB_ASCII = 0.50        # Basic ASCII letters (A-Z, a-z)
PROB_LATIN = 0.20        # Non-ASCII Latin + Cyrillic (single-width)
PROB_NUM   = 0.10        # all-digit words
PROB_CJK   = 0.025       # CJK Unified Ideograph words
# remainder -> other printable Unicode blocks
PROB_PUNCT = 0.20        # chance of mid-sentence punctuation after a word

MID_PUNCT = [',', ',', ',', ';', ':', ' —']   # weighted toward commas
END_PUNCT = ['.', '.', '.', '!', '?']          # weighted toward periods

# CJK codepoints whose glyphs resemble slash/dash/pipe/underscore - excluded
CJK_LOOKALIKE = {
	0x4E00,  # 一  dash
	0x4E28,  # 丨  pipe
	0x4E3F,  # 丿  slash
	0x4E40,  # 乀  slash-like
	0x4E41,  # 乁  slash-like
}

def _build_pool(ranges):
	"""Build list of letter chars (category L) from codepoint ranges."""
	return [chr(cp) for s, e in ranges for cp in range(s, e + 1)
			if unicodedata.category(chr(cp))[0] == 'L']

# character pools
digits = [chr(c) for c in range(0x30, 0x3A)]        # 0-9
ascii_letters = _build_pool([
	(0x0041, 0x005A), (0x0061, 0x007A),              # Basic ASCII A-Z, a-z
])
latin_cyrillic = _build_pool([
	(0x00C0, 0x024F),                                 # Latin Extended A+B
	(0x0400, 0x04FF),                                 # Cyrillic
])
cjk = [chr(cp) for cp in range(0x4E00, 0xA000) if cp not in CJK_LOOKALIKE]
other_blocks = [p for p in [
	_build_pool([(0x0370, 0x03FF)]),                  # Greek
	_build_pool([(0x0530, 0x058F)]),                  # Armenian
	_build_pool([(0x05D0, 0x05EA)]),                  # Hebrew
	_build_pool([(0x0620, 0x064A)]),                  # Arabic
	_build_pool([(0x0900, 0x097F)]),                  # Devanagari
	_build_pool([(0x0980, 0x09FF)]),                  # Bengali
	_build_pool([(0x0A80, 0x0AFF)]),                  # Gujarati
	_build_pool([(0x0B80, 0x0BFF)]),                  # Tamil
	_build_pool([(0x0E01, 0x0E3A)]),                  # Thai
	_build_pool([(0x10A0, 0x10FF)]),                  # Georgian
	_build_pool([(0x1200, 0x137F)]),                  # Ethiopic
	_build_pool([(0x3041, 0x3096)]),                  # Hiragana
	_build_pool([(0x30A1, 0x30FA)]),                  # Katakana
	_build_pool([(0xAC00, 0xD7AF)]),                  # Hangul Syllables
] if p]

def gen_word(length):
	r = random.random()
	thresh = 0.0
	thresh += PROB_NUM
	if r < thresh:
		pool = digits
	elif r < (thresh := thresh + PROB_CJK):
		pool = cjk
	elif r < (thresh := thresh + PROB_ASCII):
		pool = ascii_letters
	elif r < (thresh := thresh + PROB_LATIN):
		pool = latin_cyrillic
	else:
		pool = random.choice(other_blocks)
	return ''.join(random.choice(pool) for _ in range(length))

def gen_line():
	n = random.randint(CHARS_MIN, CHARS_MAX)        # total chars incl spaces
	words = []
	length = 0
	while length < n:
		wlen = min(random.randint(WORD_GAP_MIN, WORD_GAP_MAX), n - length)
		if words:
			length += 1                              # space before word
		if wlen <= 0:
			break
		word = gen_word(wlen)
		if words and random.random() < PROB_PUNCT:
			punct = random.choice(MID_PUNCT)
			words[-1] += punct
			length += len(punct)
		words.append(word)
		length += wlen
	line = ' '.join(words)[:n]
	line = line.rstrip(' ,;:—')
	line += random.choice(END_PUNCT)
	return line

end = time.monotonic() + duration
while time.monotonic() < end:
	print(gen_line(), flush=True)
	print(flush=True)
	if delay < DELAY_JITTER_FLOOR:
		time.sleep(delay)
	else:
		time.sleep(delay * random.uniform(DELAY_JITTER_MIN, DELAY_JITTER_MAX))


##	History:
##		- 20260628: Created.
