def is_valid(s):
    stack = []
    pairs = {')': '(', '}': '{', ']': '['}
    for c in s:
        if c in pairs.values():
            stack.append(c)
        elif c in pairs.keys():
            if not stack or stack.pop() != pairs[c]:
                return False
    return not stack

print(is_valid('()[]{}'))
print(is_valid('(]'))
print(is_valid('([)]'))