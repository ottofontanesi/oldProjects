def plot_intensity_im(Z, plot_title, c_min=-1, c_max=1, c_by=0.2, x_dim=10, y_dim=10):
    from numpy import arange
    from pylab import cm, imshow, contour, clabel, colorbar, title, show
    import matplotlib.pyplot as plt

    fig, ax = plt.subplots(figsize=(x_dim, y_dim))
    im = imshow(Z, cmap=cm.RdBu)

    # adding the Contour lines with labels
    cset = contour(Z, arange(c_min, c_max, c_by), linewidths=2, cmap=cm.Set2)
    clabel(cset, inline=True, fmt='%1.1f', fontsize=10)
    colorbar(im)
    title(plot_title)
    show()


def plot_3D_im(Z, X, Y, plot_title, x_dim=10, y_dim=10):
    from matplotlib import cm
    from matplotlib.ticker import LinearLocator, FormatStrFormatter
    import matplotlib.pyplot as plt

    fig = plt.figure(figsize=(x_dim, y_dim))
    ax = fig.add_subplot(111, projection='3d')
    surf = ax.plot_surface(X, Y, Z, rstride=1, cstride=1,
                           cmap=cm.RdBu, linewidth=0, antialiased=False)

    ax.zaxis.set_major_locator(LinearLocator(10))
    ax.zaxis.set_major_formatter(FormatStrFormatter('%.02f'))

    fig.colorbar(surf, shrink=0.5, aspect=5)
    plt.title(plot_title)
    plt.show()
