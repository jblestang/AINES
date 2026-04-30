with open("frame.raw", "rb") as f:
    data = f.read()

# count non-grey pixels
non_grey = 0
for i in range(0, len(data), 4):
    r, g, b = data[i], data[i+1], data[i+2]
    # grey is (84, 84, 84)
    if not (r == 84 and g == 84 and b == 84) and not (r == 0 and g == 0 and b == 0):
        y = (i // 4) // 256
        x = (i // 4) % 256
        non_grey += 1
        
print("Non grey pixels:", non_grey)
