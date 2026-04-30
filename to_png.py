import sys
from PIL import Image

data = open("frame.raw", "rb").read()
img = Image.frombytes("RGBA", (256, 240), data)
img.save("frame.png")
