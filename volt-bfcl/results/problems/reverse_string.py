def reverse_string(s):
    s[:] = s[::-1]

s = ['h','e','l','l','o']
reverse_string(s)
print(''.join(s))